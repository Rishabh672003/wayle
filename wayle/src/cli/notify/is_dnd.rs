use super::proxy::{connect, format_error};
use crate::cli::CliAction;

/// Executes the is-dnd command, printing `true` or `false`.
///
/// # Errors
/// Returns error if D-Bus communication fails.
pub async fn execute() -> CliAction {
    let (_connection, proxy) = connect().await?;

    let dnd = proxy
        .dnd()
        .await
        .map_err(|e| format_error("get DND state", e))?;

    println!("{dnd}");

    Ok(())
}
