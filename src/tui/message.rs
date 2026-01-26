/// Message types for TUI communication
#[derive(Clone)]
pub enum TuiMessage {
    /// Append a line to the output
    AppendLine(String),
    /// Signal that the work is complete
    WorkComplete,
    /// Request to quit (user pressed q)
    QuitRequested,
}
