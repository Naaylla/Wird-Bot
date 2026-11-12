use anyhow::Result;
use serenity::all::{
    Command, CommandInteraction, ComponentInteraction, Context, CreateCommand,
    CreateInteractionResponse, CreateInteractionResponseMessage, GuildId, Interaction, Ready,
};
use serenity::async_trait;
use serenity::prelude::EventHandler;
use tracing::{error, info, warn};

use crate::commands;
use crate::components;

pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("logged in as {}", ready.user.name);
        register_commands(&ctx).await;
    }

    async fn interaction_create(&self, ctx: Context, interaction: Interaction) {
        match interaction {
            Interaction::Command(command) => {
                if let Err(err) = run_command(&ctx, &command).await {
                    error!("command failed: {err:#}");
                    notify_command_failure(&ctx, &command).await;
                }
            }
            Interaction::Component(component) => {
                if let Err(err) = components::handle(&ctx, &component).await {
                    error!("button failed: {err:#}");
                    notify_component_failure(&ctx, &component).await;
                }
            }
            _ => {}
        }
    }
}

async fn run_command(ctx: &Context, command: &CommandInteraction) -> Result<()> {
    match command.data.name.as_str() {
        "wird" => commands::wird(ctx, command).await,
        "progress" => commands::progress(ctx, command).await,
        other => {
            warn!("unknown command: {other}");
            Ok(())
        }
    }
}

async fn register_commands(ctx: &Context) {
    let commands = vec![
        CreateCommand::new("wird").description("Show today's ayah and mark it as memorized"),
        CreateCommand::new("progress").description("Show your memorized count and daily streak"),
    ];

    let guild_id = std::env::var("GUILD_ID")
        .ok()
        .and_then(|value| value.parse::<u64>().ok());

    let result = match guild_id {
        Some(id) => GuildId::new(id).set_commands(&ctx.http, commands).await,
        None => Command::set_global_commands(&ctx.http, commands).await,
    };

    match result {
        Ok(_) => info!("slash commands registered"),
        Err(err) => error!("could not register slash commands: {err}"),
    }
}

async fn notify_command_failure(ctx: &Context, command: &CommandInteraction) {
    let _ = command
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("Something went wrong. Please try again in a moment.")
                    .ephemeral(true),
            ),
        )
        .await;
}

async fn notify_component_failure(ctx: &Context, component: &ComponentInteraction) {
    let _ = component
        .create_response(
            ctx.http.clone(),
            CreateInteractionResponse::Message(
                CreateInteractionResponseMessage::new()
                    .content("Something went wrong. Please try again in a moment.")
                    .ephemeral(true),
            ),
        )
        .await;
}
