use bytes::Bytes;
use fil_actors_runtime::{ActorError, AsActorError};

use crate::{
    interpreter::{memory::Memory, output::Outcome, Output},
    EVM_CONTRACT_BAD_JUMPDEST, EVM_CONTRACT_ILLEGAL_MEMORY_ACCESS,
    EVM_CONTRACT_INVALID_INSTRUCTION,
};

use {
    super::memory::get_memory_region,
    crate::interpreter::Bytecode,
    crate::interpreter::{ExecutionState, System, U256},
    fil_actors_runtime::runtime::Runtime,
};

#[inline]
pub fn nop(_state: &mut ExecutionState, _system: &System<impl Runtime>) -> Result<(), ActorError> {
    Ok(())
}

#[inline]
pub fn invalid(
    _state: &mut ExecutionState,
    _system: &System<impl Runtime>,
) -> Result<(), ActorError> {
    Err(ActorError::unchecked(EVM_CONTRACT_INVALID_INSTRUCTION, "invalid instruction".into()))
}

#[inline]
pub fn ret(
    state: &mut ExecutionState,
    _system: &System<impl Runtime>,
    offset: U256,
    size: U256,
) -> Result<Output, ActorError> {
    exit(&mut state.memory, offset, size, Outcome::Return)
}

#[inline]
pub fn revert(
    state: &mut ExecutionState,
    _system: &System<impl Runtime>,
    offset: U256,
    size: U256,
) -> Result<Output, ActorError> {
    exit(&mut state.memory, offset, size, Outcome::Revert)
}

#[inline]
pub fn stop(
    _state: &mut ExecutionState,
    _system: &System<impl Runtime>,
) -> Result<Output, ActorError> {
    Ok(Output { return_data: Bytes::new(), outcome: Outcome::Return })
}

#[inline]
fn exit(
    memory: &mut Memory,
    offset: U256,
    size: U256,
    status: Outcome,
) -> Result<Output, ActorError> {
    Ok(Output {
        outcome: status,
        return_data: super::memory::get_memory_region(memory, offset, size)?
            .map(|region| memory[region.offset..region.offset + region.size.get()].to_vec().into())
            .unwrap_or_default(),
    })
}

#[inline]
pub fn returndatasize(
    state: &mut ExecutionState,
    _system: &System<impl Runtime>,
) -> Result<U256, ActorError> {
    Ok(U256::from(state.return_data.len()))
}

#[inline]
pub fn returndatacopy(
    state: &mut ExecutionState,
    _system: &System<impl Runtime>,
    mem_index: U256,
    input_index: U256,
    size: U256,
) -> Result<(), ActorError> {
    let region = get_memory_region(&mut state.memory, mem_index, size)?;

    let src: usize = input_index
        .try_into()
        .context_code(EVM_CONTRACT_ILLEGAL_MEMORY_ACCESS, "returndatacopy index exceeds max u32")?;
    if src > state.return_data.len() {
        return Err(ActorError::unchecked(
            EVM_CONTRACT_ILLEGAL_MEMORY_ACCESS,
            format!(
                "returndatacopy start {} exceeds return-data length {}",
                src,
                state.return_data.len()
            ),
        ));
    }

    let end = src
        .checked_add(region.as_ref().map(|r| r.size.get()).unwrap_or(0))
        .context_code(EVM_CONTRACT_ILLEGAL_MEMORY_ACCESS, "returndatacopy end exceeds max u32")?;

    if end > state.return_data.len() {
        return Err(ActorError::unchecked(
            EVM_CONTRACT_ILLEGAL_MEMORY_ACCESS,
            format!(
                "returndatacopy end {} exceeds return-data length {}",
                src,
                state.return_data.len()
            ),
        ));
    }

    if let Some(region) = region {
        state.memory[region.offset..region.offset + region.size.get()]
            .copy_from_slice(&state.return_data[src..src + region.size.get()]);
    }

    Ok(())
}

#[inline]
pub fn jump(bytecode: &Bytecode, _pc: usize, dest: U256) -> Result<usize, ActorError> {
    let dst = dest.try_into().context_code(EVM_CONTRACT_BAD_JUMPDEST, "jumpdest exceeds u32")?;
    if !bytecode.valid_jump_destination(dst) {
        return Err(ActorError::unchecked(
            EVM_CONTRACT_BAD_JUMPDEST,
            format!("jumpdest {dst} is invalid"),
        ));
    }
    // skip the JMPDEST noop sled
    Ok(dst + 1)
}

#[inline]
pub fn jumpi(bytecode: &Bytecode, pc: usize, dest: U256, test: U256) -> Result<usize, ActorError> {
    if !test.is_zero() {
        let dst =
            dest.try_into().context_code(EVM_CONTRACT_BAD_JUMPDEST, "jumpdest exceeds u32")?;
        if !bytecode.valid_jump_destination(dst) {
            return Err(ActorError::unchecked(
                EVM_CONTRACT_BAD_JUMPDEST,
                format!("jumpdest {dst} is invalid"),
            ));
        }
        // skip the JMPDEST noop sled
        Ok(dst + 1)
    } else {
        Ok(pc + 1)
    }
}

#[cfg(test)]
mod tests {
    use crate::do_test;
    use crate::interpreter::U256;
    use crate::EVM_CONTRACT_BAD_JUMPDEST;

    #[test]
    fn test_jump() {
        do_test!(
            rt,
            m,
            vec![
                0x56, // JUMP
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
            ],
            {
                m.state.stack.push(U256::from(2)).unwrap();
                let result = m.step();
                assert!(result.is_ok(), "execution step failed");
                assert_eq!(m.state.stack.len(), 0);
                assert_eq!(m.pc, 3, "pc has not advanced to 3");
            }
        );
    }

    #[test]
    fn test_jump_err() {
        do_test!(
            rt,
            m,
            vec![
                0x56, // JUMP
                0x63, // PUSH4 -- garbage
                0x01, // garbage
                0x02, // garbage
                0x03, // garbage
                0x04, // garbage
            ],
            {
                m.state.stack.push(U256::from(2)).unwrap();
                let result = m.step();
                assert_eq!(m.state.stack.len(), 0);
                assert!(result.is_err(), "execution step succeeded");
                assert_eq!(result.err().unwrap().exit_code(), EVM_CONTRACT_BAD_JUMPDEST);
            }
        );
    }

    #[test]
    fn test_jump_err2() {
        do_test!(
            rt,
            m,
            vec![
                0x56, // JUMP
                0x63, // PUSH4 -- garbage
                0x01, // garbage
                0x02, // garbage
                0x03, // garbage
                0x04, // garbage
            ],
            {
                m.state.stack.push(U256::from(123)).unwrap();
                let result = m.step();
                assert_eq!(m.state.stack.len(), 0);
                assert!(result.is_err(), "execution step succeeded");
                assert_eq!(result.err().unwrap().exit_code(), EVM_CONTRACT_BAD_JUMPDEST);
            }
        );
    }

    #[test]
    fn test_jump_err3() {
        do_test!(
            rt,
            m,
            vec![
                0x56, // JUMP
                0x63, // PUSH4 -- garbage
                0x5b, // garbage
                0x5b, // garbage
                0x5b, // garbage
                0x5b, // garbage
            ],
            {
                m.state.stack.push(U256::from(2)).unwrap();
                let result = m.step();
                assert_eq!(m.state.stack.len(), 0);
                assert!(result.is_err(), "execution step succeeded");
                assert_eq!(result.err().unwrap().exit_code(), EVM_CONTRACT_BAD_JUMPDEST);
            }
        );
    }

    #[test]
    fn test_jumpi_t() {
        do_test!(
            rt,
            m,
            vec![
                0x57, // JUMPI
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
            ],
            {
                m.state.stack.push(U256::from(1)).unwrap();
                m.state.stack.push(U256::from(2)).unwrap();
                let result = m.step();
                assert!(result.is_ok(), "execution step failed");
                assert_eq!(m.state.stack.len(), 0);
                assert_eq!(m.pc, 3, "pc has not advanced to 3");
            }
        );
    }

    #[test]
    fn test_jumpi_f() {
        do_test!(
            rt,
            m,
            vec![
                0x57, // JUMPI
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
            ],
            {
                m.state.stack.push(U256::from(0)).unwrap();
                m.state.stack.push(U256::from(2)).unwrap();
                let result = m.step();
                assert!(result.is_ok(), "execution step failed");
                assert_eq!(m.state.stack.len(), 0);
                assert_eq!(m.pc, 1, "pc has not advanced to 1");
            }
        );
    }

    #[test]
    fn test_jumpi_err() {
        do_test!(
            rt,
            m,
            vec![
                0x57, // JUMPI
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
                0x5b, // JMPDEST -- noop
            ],
            {
                m.state.stack.push(U256::from(1)).unwrap();
                m.state.stack.push(U256::from(123)).unwrap();
                let result = m.step();
                assert_eq!(m.state.stack.len(), 0);
                assert!(result.is_err(), "execution step succeeded");
                assert_eq!(result.err().unwrap().exit_code(), EVM_CONTRACT_BAD_JUMPDEST);
            }
        );
    }

    #[test]
    fn test_pc() {
        do_test!(
            rt,
            m,
            vec![
                0x58, // PC
                0x5b, // JMPDEST -- noop
            ],
            {
                let result = m.step();
                assert!(result.is_ok(), "execution step failed");
                assert_eq!(m.state.stack.len(), 1);
                assert_eq!(m.state.stack.pop().unwrap(), U256::zero());
                assert_eq!(m.pc, 1, "pc has not advanced to 1");
            }
        );
    }
}
