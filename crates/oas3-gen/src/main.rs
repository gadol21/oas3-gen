use clap::Parser;

use oas3_gen::ui::{Cli, Colors, Commands, ListCommands, colors, commands};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
  let cli = Cli::parse();
  let colors = Colors::new(colors::colors_enabled(cli.color), colors::detect_theme(cli.theme));

  match cli.command {
    Commands::List { list_command } => match list_command {
      ListCommands::Operations { input } => commands::list_operations(&input, &colors).await?,
    },
    Commands::Generate(command) => {
      let config = commands::GenerateConfig::from_command(command)?;
      commands::generate_code(config, &colors).await?;
    }
  }

  Ok(())
}
