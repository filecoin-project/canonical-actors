// A namespace for helpers that build and emit verified registry events.

use crate::ActorError;
use crate::DataCap;
use fil_actors_runtime::runtime::Runtime;
use fil_actors_runtime::EventBuilder;
use fvm_shared::ActorID;

/// Indicates a new value for a verifier's datacap balance.
/// Note that receiving this event does not necessarily mean the balance has changed.
/// The value is in datacap whole units (not TokenAmount).
pub fn verifier_balance(
    rt: &impl Runtime,
    verifier: ActorID,
    new_balance: &DataCap,
) -> Result<(), ActorError> {
    rt.emit_event(
        &EventBuilder::new()
            .event_type("verifier-balance")
            .field_indexed("verifier", &verifier)
            .field("balance", new_balance)
            .build()?,
    )
}
