pub mod cli;
pub mod colors;
#[cfg(feature = "async")]
pub mod commands;

pub use cli::{Cli, Commands, EnumCaseMode, GenerateCommand, GenerateMode, ListCommands};
pub use colors::Colors;

#[allow(dead_code)]
fn term_width() -> u16 {
  if let Ok((width, _)) = crossterm::terminal::size() {
    width
  } else {
    80
  }
}
