use crate::id::CommandName;

/// Trait implemented by command types. The derive macro generates the
/// `NAME` constant and `command_name()` discriminator from variant names
/// (kebab-case, prefixed).
///
/// For enum commands, `NAME` is not available (each variant has a different
/// name), so only `command_name(&self)` is used. For struct commands, `NAME`
/// provides a type-level constant that generated dispatch code can use
/// without constructing a value.
pub trait Command: Send + 'static {
    /// The command name as a static string, available at the type level.
    ///
    /// For enum commands this returns the enum-level prefix (not a specific
    /// variant), so dispatch code should use `command_name(&self)` instead.
    /// For struct commands this is the canonical name used in dispatch.
    const NAME: &'static str;

    fn command_name(&self) -> CommandName {
        CommandName::new(Self::NAME)
    }
}
