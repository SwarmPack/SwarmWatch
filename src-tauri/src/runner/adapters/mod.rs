pub mod claude;
pub mod cline;
pub mod cursor;
pub mod vscode;
pub mod windsurf;

#[cfg(test)]
mod vscode_tests;

pub use claude::ClaudeAdapter;
pub use cline::ClineAdapter;
pub use cursor::CursorAdapter;
pub use vscode::VsCodeAdapter;
pub use windsurf::WindsurfAdapter;
