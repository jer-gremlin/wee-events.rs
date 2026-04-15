use crate::id::CommandName;

/// Trait implemented by command types.
///
/// Commands are inherently serializable — they cross network and process
/// boundaries. The `Serialize` supertrait ensures any command can be
/// encoded for transport without requiring adapter-specific bounds.
///
/// `NAME` provides a type-level constant for static dispatch: generated
/// adapter code matches on `<C as Command>::NAME` before deserializing,
/// avoiding speculative deserialization.
pub trait Command: serde::Serialize + Send + 'static {
    /// The command name as a static string, available at the type level.
    ///
    /// For struct commands this is the canonical name used in dispatch.
    const NAME: &'static str;

    fn command_name(&self) -> CommandName {
        CommandName::new(Self::NAME)
    }
}
