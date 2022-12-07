use fil_actors_runtime::runtime::{builtins::Type, Runtime};
use fvm_ipld_encoding::RawBytes;
use fvm_shared::{address::Address, econ::TokenAmount, sys::SendFlags};
use num_traits::FromPrimitive;

use crate::interpreter::{
    instructions::call::CallKind,
    precompiles::{
        parameter::{read_right_pad, Parameter},
        NativeType,
    },
    System, U256,
};

use super::{parameter::U256Reader, PrecompileContext, PrecompileError, PrecompileResult};

/// Read right padded BE encoded low u64 ID address from a u256 word.
/// Returns variant of [`BuiltinType`] encoded as a u256 word.
pub(super) fn get_actor_type<RT: Runtime>(
    system: &mut System<RT>,
    input: &[u8],
    _: PrecompileContext,
) -> PrecompileResult {
    const LAST_SYSTEM_ACTOR_ID: u64 = 32;

    let id_bytes: [u8; 32] = read_right_pad(input, 32).as_ref().try_into().unwrap();
    let id = Parameter::<u64>::try_from(&id_bytes)?.0;

    if id < LAST_SYSTEM_ACTOR_ID {
        // known to be system actors
        Ok(NativeType::System.word_vec())
    } else {
        // resolve type from code CID
        let builtin_type = system
            .rt
            .get_actor_code_cid(&id)
            .and_then(|cid| system.rt.resolve_builtin_actor_type(&cid));

        let builtin_type = match builtin_type {
            Some(t) => match t {
                Type::Account => NativeType::Account,
                Type::System => NativeType::System,
                Type::Embryo => NativeType::Embryo,
                Type::EVM => NativeType::EVMContract,
                Type::Miner => NativeType::StorageProvider,
                // Others
                Type::PaymentChannel | Type::Multisig => NativeType::OtherTypes,
                // Singletons
                Type::Market
                | Type::Power
                | Type::Init
                | Type::Cron
                | Type::Reward
                | Type::VerifiedRegistry
                | Type::DataCap
                | Type::EAM => NativeType::System,
            },
            None => NativeType::NonExistent,
        };

        Ok(builtin_type.word_vec())
    }
}

/// Params:
///
/// | Param            | Value                     |
/// |------------------|---------------------------|
/// | randomness_type  | U256 - low i32: `Chain`(0) OR `Beacon`(1) |
/// | personalization  | U256 - low i64             |
/// | randomness_epoch | U256 - low i64             |
/// | entropy_length   | U256 - low u32             |
/// | entropy          | input\[32..] (right padded)|
///
/// any bytes in between values are ignored
///
/// Returns empty array if invalid randomness type
/// Errors if unable to fetch randomness
pub(super) fn get_randomness<RT: Runtime>(
    system: &mut System<RT>,
    input: &[u8],
    _: PrecompileContext,
) -> PrecompileResult {
    let mut input_params = U256Reader::new(input);

    #[derive(num_derive::FromPrimitive)]
    #[repr(i32)]
    enum RandomnessType {
        Chain = 0,
        Beacon = 1,
    }

    let randomness_type = RandomnessType::from_i32(input_params.next_param_padded::<i32>()?);
    let personalization = input_params.next_param_padded::<i64>()?;
    let rand_epoch = input_params.next_param_padded::<i64>()?;
    let entropy_len = input_params.next_param_padded::<u32>()?;

    debug_assert_eq!(input_params.chunks_read(), 4);

    let entropy = read_right_pad(input_params.remaining_slice(), entropy_len as usize);

    let randomness = match randomness_type {
        Some(RandomnessType::Chain) => system
            .rt
            .user_get_randomness_from_chain(personalization, rand_epoch, &entropy)
            .map(|a| a.to_vec()),
        Some(RandomnessType::Beacon) => system
            .rt
            .user_get_randomness_from_beacon(personalization, rand_epoch, &entropy)
            .map(|a| a.to_vec()),
        None => Ok(Vec::new()),
    };

    randomness.map_err(|_| PrecompileError::InvalidInput)
}

/// Read BE encoded low u64 ID address from a u256 word
/// Looks up and returns the other address (encoded f2 or f4 addresses) of an ID address, returning empty array if not found
pub(super) fn lookup_address<RT: Runtime>(
    system: &mut System<RT>,
    input: &[u8],
    _: PrecompileContext,
) -> PrecompileResult {
    let mut id_bytes = U256Reader::new(input);
    let id = id_bytes.next_param_padded::<u64>()?;

    let address = system.rt.lookup_address(id);
    let ab = match address {
        Some(a) => a.to_bytes(),
        None => Vec::new(),
    };
    Ok(ab)
}

/// Reads a FIL encoded address
/// Resolves a FIL encoded address into an ID address
/// returns BE encoded u64 or empty array if nothing found
pub(super) fn resolve_address<RT: Runtime>(
    system: &mut System<RT>,
    input: &[u8],
    _: PrecompileContext,
) -> PrecompileResult {
    let mut input_params = U256Reader::new(input);

    let len = input_params.next_param_padded::<u32>()? as usize;
    let addr = match Address::from_bytes(&read_right_pad(input_params.remaining_slice(), len)) {
        Ok(o) => o,
        Err(_) => return Ok(Vec::new()),
    };
    Ok(system.rt.resolve_address(&addr).map(|a| a.to_be_bytes().to_vec()).unwrap_or_default())
}

/// Errors:
///    TODO should just give 0s?
/// - `IncorrectInputSize` if offset is larger than total input length
/// - `InvalidInput` if supplied address bytes isnt a filecoin address
///
/// Returns:
///
/// `[int256 exit_code, uint codec, uint offset, uint size, []bytes <actor return value>]`
///
/// for exit_code:
/// - negative values are system errors
/// - positive are user errors (from the called actor)
/// - 0 is success
pub(super) fn call_actor<RT: Runtime>(
    system: &mut System<RT>,
    input: &[u8],
    ctx: PrecompileContext,
) -> PrecompileResult {
    // ----- Input Parameters -------

    if ctx.call_type != CallKind::DelegateCall {
        return Err(PrecompileError::CallForbidden);
    }

    let mut input_params = U256Reader::new(input);

    let method: u64 = input_params.next_param_padded()?;

    let value: U256 = input_params.next_padded().into();

    let flags: u64 = input_params.next_param_padded()?;
    let flags = SendFlags::from_bits(flags).ok_or(PrecompileError::InvalidInput)?;
    if !flags.read_only() && ctx.is_readonly {
        // read only is required to be set on send
        // REMOVEME silently override maybe?
        return Err(PrecompileError::CallActorError(
            crate::interpreter::StatusCode::StaticModeViolation,
        ));
    }

    let codec: u64 = input_params.next_param_padded()?;
    // TODO only CBOR for now
    if codec != fvm_ipld_encoding::DAG_CBOR {
        return Err(PrecompileError::InvalidInput);
    }

    let address_size = input_params.next_param_padded::<u32>()? as usize;
    let send_data_size = input_params.next_param_padded::<u32>()? as usize;

    // ------ Begin Call -------

    let result = {
        let start = input_params.remaining_slice();
        let bytes = read_right_pad(start, send_data_size + address_size);

        let input_data = &bytes[..send_data_size];
        let address = &bytes[send_data_size..send_data_size + address_size];
        let address = Address::from_bytes(address).map_err(|_| PrecompileError::InvalidInput)?;

        system.send_generalized(
            &address,
            method,
            RawBytes::from(input_data.to_vec()),
            TokenAmount::from(&value),
            ctx.gas_limit,
            flags,
        )
    };

    // ------ Build Output -------

    let output = {
        // negative values are syscall errors
        // positive values are user/actor errors
        // success is 0
        let (exit_code, data) = match result {
            Err(mut ae) => {
                // TODO handle revert
                // TODO https://github.com/filecoin-project/ref-fvm/issues/1020
                // put error number from call into revert
                let exit_code = U256::from(ae.exit_code().value());

                // no return only exit code
                (exit_code, ae.take_data())
            }
            Ok(ret) => (U256::zero(), ret),
        };

        const NUM_OUTPUT_PARAMS: u32 = 4;

        // codec of return data
        // TODO hardcoded to CBOR for now
        let codec = U256::from(fvm_ipld_encoding::DAG_CBOR);
        let offset = U256::from(NUM_OUTPUT_PARAMS * 32);
        let size = U256::from(data.len() as u32);

        let mut output = Vec::with_capacity(NUM_OUTPUT_PARAMS as usize * 32 + data.len());
        output.extend_from_slice(&exit_code.to_bytes());
        output.extend_from_slice(&codec.to_bytes());
        output.extend_from_slice(&offset.to_bytes());
        output.extend_from_slice(&size.to_bytes());
        // NOTE:
        // we dont pad out to 32 bytes here, the idea being that users will already be in the "everythig is bytes" mode
        // and will want re-pack align and whatever else by themselves
        output.extend_from_slice(data.bytes());
        output
    };

    Ok(output)
}
