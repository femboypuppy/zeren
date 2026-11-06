//! The subcommands that run the engine's own code ([`zeren_cli::EngineCommand`]),
//! in a binary that links it: `zeren` itself, and `zeren-engine`, the helper
//! an interface without the engine hands them to. Logging is set up by
//! [`zeren_cli::run`] before these run.

use zeren_cli::EngineCommand;

use crate::{auth_cli, engine_config_from_env, ipc_cli, plugin_cli};

pub fn run_engine_command(command: EngineCommand) -> anyhow::Result<()> {
    match command {
        EngineCommand::Headless => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(async {
                let engine = zeren_engine::Engine::new(engine_config_from_env());
                engine.run().await
            })
        }
        EngineCommand::Login => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(auth_cli::login(engine_config_from_env()))
        }
        EngineCommand::Logout => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(auth_cli::logout(engine_config_from_env()))
        }
        EngineCommand::Status => {
            let runtime = tokio::runtime::Runtime::new()?;
            runtime.block_on(auth_cli::status(engine_config_from_env()))
        }
        EngineCommand::Sync => {
            let runtime = tokio::runtime::Runtime::new()?;
            let config = engine_config_from_env();
            runtime.block_on(ipc_cli::sync_cli(config.ipc_port, &config.data_dir))
        }
        EngineCommand::Quiesce => {
            let runtime = tokio::runtime::Runtime::new()?;
            let config = engine_config_from_env();
            runtime.block_on(ipc_cli::quiesce_cli(config.ipc_port, &config.data_dir))
        }
        EngineCommand::Mcp => crate::run_mcp(),
        EngineCommand::Plugin(command) => {
            let runtime = tokio::runtime::Runtime::new()?;
            let config = engine_config_from_env();
            runtime.block_on(plugin_cli::run(command, config.ipc_port, &config.data_dir))
        }
        EngineCommand::Template(command) => {
            let runtime = tokio::runtime::Runtime::new()?;
            let config = engine_config_from_env();
            runtime.block_on(plugin_cli::run_template(
                command,
                config.ipc_port,
                &config.data_dir,
            ))
        }
        EngineCommand::Engine => crate::background::run(),
    }
}
