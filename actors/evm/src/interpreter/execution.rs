#![allow(dead_code)]

use fvm_shared::address::Address as FilecoinAddress;

use super::address::EthAddress;
use {
    super::instructions,
    super::opcode::OpCode,
    super::StatusCode,
    crate::interpreter::memory::Memory,
    crate::interpreter::stack::Stack,
    crate::interpreter::{Bytecode, Output, System},
    bytes::Bytes,
    fil_actors_runtime::runtime::Runtime,
};

/// EVM execution runtime.
#[derive(Clone, Debug)]
pub struct ExecutionState {
    pub stack: Stack,
    pub memory: Memory,
    pub input_data: Bytes,
    pub return_data: Bytes,
    pub output_data: Bytes,
    /// Indicates whether the contract called SELFDESTRUCT, providing the beneficiary.
    pub selfdestroyed: Option<FilecoinAddress>,
    /// The EVM address of the caller.
    pub caller: EthAddress,
    /// The EVM address of the receiver.
    pub receiver: EthAddress,
}

impl ExecutionState {
    pub fn new(caller: EthAddress, receiver: EthAddress, input_data: Bytes) -> Self {
        Self {
            stack: Stack::new(),
            memory: Memory::default(),
            input_data,
            return_data: Default::default(),
            output_data: Bytes::new(),
            selfdestroyed: None,
            caller,
            receiver,
        }
    }
}

pub struct Machine<'r, 'a, RT: Runtime + 'a> {
    pub system: &'r mut System<'a, RT>,
    pub state: &'r mut ExecutionState,
    pub bytecode: &'r Bytecode,
    pub pc: usize,
    pub reverted: bool,
}

enum ControlFlow {
    Continue,
    Jump,
    Exit,
}

type Instruction<M> = fn(*mut M) -> Result<ControlFlow, StatusCode>;

macro_rules! def_opcodes {
    ($($op:ident: $body:tt)*) => {
        def_ins_raw! {
            UNDEFINED(_m) {
                Err(StatusCode::UndefinedInstruction)
            }
        }
        $(def_ins! { $op $body })*
        def_jmptable! {
            $($op)*
        }
    }
}

macro_rules! def_jmptable {
    ($($op:ident)*) => {
        const fn jmptable() -> [Instruction<Machine<'r, 'a, RT>>; 256] {
            let mut table: [Instruction<Machine::<'r, 'a, RT>>; 256] = [Machine::<'r, 'a, RT>::UNDEFINED; 256];
            $(table[OpCode::$op as usize] = Machine::<'r, 'a, RT>::$op;)*
            table
        }
    }
}

macro_rules! def_ins {
    ($ins:ident {intrinsic}) => {
        def_ins_intrinsic! { $ins }
    };

    ($ins:ident {=> $expr:expr}) => {
        def_ins_raw! { $ins (_m) { $expr } }
    };

    ($ins:ident {($arg:ident) => $body:block}) => {
        def_ins_raw! { $ins ($arg) $body }
    };
}

macro_rules! def_ins_raw {
    ($ins:ident ($arg:ident) $body:block) => {
        #[allow(non_snake_case)]
        fn $ins(p: *mut Self) -> Result<ControlFlow, StatusCode> {
            // SAFETY: macro ensures that mut pointer is taken directly from a mutable borrow, used once, then goes out of scope immediately after
            let $arg: &mut Self = unsafe { p.as_mut().unwrap() };
            $body
        }
    };
}

macro_rules! def_ins_intrinsic {
    ($ins:ident) => {
        def_ins_raw! {
            $ins (m) {
                instructions::$ins(m)?;
                Ok(ControlFlow::Continue)
            }
        }
    };
}

impl<'r, 'a, RT: Runtime + 'r> Machine<'r, 'a, RT> {
    pub fn new(
        system: &'r mut System<'a, RT>,
        state: &'r mut ExecutionState,
        bytecode: &'r Bytecode,
    ) -> Self {
        Machine { system, state, bytecode, pc: 0, reverted: false }
    }

    pub fn execute(&mut self) -> Result<(), StatusCode> {
        loop {
            if self.pc >= self.bytecode.len() {
                break;
            }

            match self.step()? {
                ControlFlow::Continue => {
                    self.pc += 1;
                }
                ControlFlow::Jump => {}
                ControlFlow::Exit => {
                    break;
                }
            };
        }

        Ok(())
    }

    fn step(&mut self) -> Result<ControlFlow, StatusCode> {
        let op = OpCode::try_from(self.bytecode[self.pc])?;
        Self::JMPTABLE[op as usize](self)
    }

    def_opcodes! {
        STOP: {=> Ok(ControlFlow::Exit)}

        // primops
        ADD: {intrinsic}
        MUL: {intrinsic}
        SUB: {intrinsic}
        DIV: {intrinsic}
        SDIV: {intrinsic}
        MOD: {intrinsic}
        SMOD: {intrinsic}
        ADDMOD: {intrinsic}
        MULMOD: {intrinsic}
        EXP: {intrinsic}
        SIGNEXTEND: {intrinsic}
        LT: {intrinsic}
        GT: {intrinsic}
        SLT: {intrinsic}
        SGT: {intrinsic}
        EQ: {intrinsic}
        ISZERO: {intrinsic}
        AND: {intrinsic}
        OR: {intrinsic}
        XOR: {intrinsic}
        NOT: {intrinsic}
        BYTE: {intrinsic}
        SHL: {intrinsic}
        SHR: {intrinsic}
        SAR: {intrinsic}

        // std call convenction functionoids
        KECCAK256: {intrinsic}
        ADDRESS: {intrinsic}
        BALANCE: {intrinsic}
        ORIGIN: {intrinsic}
        CALLER: {intrinsic}
        CALLVALUE: {intrinsic}
        CALLDATALOAD: {intrinsic}
        CALLDATASIZE: {intrinsic}
        CALLDATACOPY: {intrinsic}
        CODESIZE: {intrinsic}
        CODECOPY: {intrinsic}
        GASPRICE: {intrinsic}
        EXTCODESIZE: {intrinsic}
        EXTCODECOPY: {intrinsic}
        RETURNDATASIZE: {intrinsic}
        RETURNDATACOPY: {intrinsic}
        EXTCODEHASH: {intrinsic}
        BLOCKHASH: {intrinsic}
        COINBASE: {intrinsic}
        TIMESTAMP: {intrinsic}
        NUMBER: {intrinsic}
        DIFFICULTY: {intrinsic}
        GASLIMIT: {intrinsic}
        CHAINID: {intrinsic}
        BASEFEE: {intrinsic}
        SELFBALANCE: {intrinsic}
        MLOAD: {intrinsic}
        MSTORE: {intrinsic}
        MSTORE8: {intrinsic}
        SLOAD: {intrinsic}
        SSTORE: {intrinsic}
        MSIZE: {intrinsic}
        GAS: {intrinsic}

        // stack ops
        POP: {intrinsic}

        // push variants
        PUSH1: {intrinsic}
        PUSH2: {intrinsic}
        PUSH3: {intrinsic}
        PUSH4: {intrinsic}
        PUSH5: {intrinsic}
        PUSH6: {intrinsic}
        PUSH7: {intrinsic}
        PUSH8: {intrinsic}
        PUSH9: {intrinsic}
        PUSH10: {intrinsic}
        PUSH11: {intrinsic}
        PUSH12: {intrinsic}
        PUSH13: {intrinsic}
        PUSH14: {intrinsic}
        PUSH15: {intrinsic}
        PUSH16: {intrinsic}
        PUSH17: {intrinsic}
        PUSH18: {intrinsic}
        PUSH19: {intrinsic}
        PUSH20: {intrinsic}
        PUSH21: {intrinsic}
        PUSH22: {intrinsic}
        PUSH23: {intrinsic}
        PUSH24: {intrinsic}
        PUSH25: {intrinsic}
        PUSH26: {intrinsic}
        PUSH27: {intrinsic}
        PUSH28: {intrinsic}
        PUSH29: {intrinsic}
        PUSH30: {intrinsic}
        PUSH31: {intrinsic}
        PUSH32: {intrinsic}

        // dup variants
        DUP1: {intrinsic}
        DUP2: {intrinsic}
        DUP3: {intrinsic}
        DUP4: {intrinsic}
        DUP5: {intrinsic}
        DUP6: {intrinsic}
        DUP7: {intrinsic}
        DUP8: {intrinsic}
        DUP9: {intrinsic}
        DUP10: {intrinsic}
        DUP11: {intrinsic}
        DUP12: {intrinsic}
        DUP13: {intrinsic}
        DUP14: {intrinsic}
        DUP15: {intrinsic}
        DUP16: {intrinsic}

        // swap variants
        SWAP1: {intrinsic}
        SWAP2: {intrinsic}
        SWAP3: {intrinsic}
        SWAP4: {intrinsic}
        SWAP5: {intrinsic}
        SWAP6: {intrinsic}
        SWAP7: {intrinsic}
        SWAP8: {intrinsic}
        SWAP9: {intrinsic}
        SWAP10: {intrinsic}
        SWAP11: {intrinsic}
        SWAP12: {intrinsic}
        SWAP13: {intrinsic}
        SWAP14: {intrinsic}
        SWAP15: {intrinsic}
        SWAP16: {intrinsic}

        // event logs
        LOG0: {intrinsic}
        LOG1: {intrinsic}
        LOG2: {intrinsic}
        LOG3: {intrinsic}
        LOG4: {intrinsic}

        // create variants
        CREATE: {intrinsic}
        CREATE2: {intrinsic}

        // call variants
        CALL: {intrinsic}
        CALLCODE: {intrinsic}
        DELEGATECALL: {intrinsic}
        STATICCALL: {intrinsic}

        // control flow magic
        JUMPDEST: {=> Ok(ControlFlow::Continue)} // noop marker opcode for valid jumps addresses

        JUMP: {(m) => {
            if let Some(dest) = instructions::JUMP(m)? {
                m.pc = dest;
                Ok(ControlFlow::Jump)
            } else {
                // cant happen, unless it's a cosmic ray
                Err(StatusCode::Failure)
            }
        }}

        JUMPI: {(m) => {
            if let Some(dest) = instructions::JUMPI(m)? {
                m.pc = dest;
                Ok(ControlFlow::Jump)
            } else {
                Ok(ControlFlow::Continue)
            }
        }}

        PC: {(m) => {
            instructions::PC(m)?;
            Ok(ControlFlow::Continue)
        }}

        RETURN: {(m) => {
            instructions::RETURN(m)?;
            Ok(ControlFlow::Exit)
        }}

        REVERT: {(m) => {
            instructions::REVERT(m)?;
            m.reverted = true;
            Ok(ControlFlow::Exit)
        }}

        SELFDESTRUCT: {(m) => {
            instructions::SELFDESTRUCT(m)?;
            Ok(ControlFlow::Exit) // selfdestruct halts the current context
        }}

        INVALID: {=> Err(StatusCode::InvalidInstruction)}
    }

    const JMPTABLE: [Instruction<Machine<'r, 'a, RT>>; 256] = Machine::<'r, 'a, RT>::jmptable();
}

pub fn execute(
    bytecode: &Bytecode,
    runtime: &mut ExecutionState,
    system: &mut System<impl Runtime>,
) -> Result<Output, StatusCode> {
    let mut m = Machine::new(system, runtime, bytecode);
    m.execute()?;
    Ok(Output {
        reverted: m.reverted,
        status_code: StatusCode::Success,
        output_data: m.state.output_data.clone(),
        selfdestroyed: m.state.selfdestroyed,
    })
}
