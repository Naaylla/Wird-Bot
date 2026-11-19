use anyhow::Result;
use serenity::all::{
    Command, CommandInteraction, CommandOptionType, ComponentInteraction, Context, CreateCommand,
    CreateCommandOption, CreateInteractionResponse, CreateInteractionResponseMessage,
    CreateMessage, Guild, GuildId, Interaction, Ready,
};
use serenity::async_trait;
use serenity::prelude::EventHandler;
use tracing::{error, info, warn};

use crate::commands;
use crate::components;
use crate::ui;

pub struct Handler;

#[async_trait]
impl EventHandler for Handler {
    async fn ready(&self, ctx: Context, ready: Ready) {
        info!("logged in as {}", ready.user.name);

        if let Err(err) = Command::set_global_commands(&ctx.http, Vec::new()).await {
            warn!("could not clear global commands: {err}");
        }

        for guild in &ready.guilds {
            register_commands(&ctx, guild.id).await;
        }
    }

    async fn guild_create(&self, ctx: Context, guild: Guild, is_new: Option<bool>) {
        register_commands(&ctx, guild.id).await;

        if is_new == Some(true) {
            send_intro(&ctx, &guild).await;
        }
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
        "plan" => commands::plan(ctx, command).await,
        "wird" => commands::wird(ctx, command).await,
        "progress" => commands::progress(ctx, command).await,
        "leaderboard" => commands::leaderboard(ctx, command).await,
        "reset" => commands::reset(ctx, command).await,
        "remind" => commands::remind(ctx, command).await,
        "help" => commands::help(ctx, command).await,
        other => {
            warn!("unknown command: {other}");
            Ok(())
        }
    }
}

fn command_definitions() -> Vec<CreateCommand> {
    vec![
        CreateCommand::new("plan")
            .description("Choose your personal memorization plan")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "mode",
                    "Choose the whole Qur'an or one surah",
                )
                .required(true)
                .add_string_choice("Full Qur'an", "quran")
                .add_string_choice("One surah", "surah"),
            ),
        CreateCommand::new("wird")
            .description("Show today's ayahs together and mark each one memorized"),
        CreateCommand::new("progress").description("Show your memorized count and daily streak"),
        CreateCommand::new("leaderboard")
            .description("Show the server's daily memorization streak leaderboard"),
        CreateCommand::new("reset").description("Reset your progress and streak in this server"),
        CreateCommand::new("remind")
            .description("Get a daily DM when your wird is unfinished")
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "time",
                    "Time in 24-hour HH:MM, e.g. 19:30",
                )
                .min_length(4)
                .max_length(5),
            )
            .add_option(
                CreateCommandOption::new(
                    CommandOptionType::String,
                    "offset",
                    "Your UTC offset, e.g. +03:00 or +05:30",
                )
                .min_length(6)
                .max_length(6),
            )
            .add_option(CreateCommandOption::new(
                CommandOptionType::Boolean,
                "disable",
                "Turn your daily reminder off",
            )),
        CreateCommand::new("help").description("How Wird works and what each command does"),
    ]
}

async fn send_intro(ctx: &Context, guild: &Guild) {
    let Some(channel_id) = guild.system_channel_id else {
        warn!(
            "guild {} has no system channel; skipping the introduction panel",
            guild.id
        );
        return;
    };

    let message = CreateMessage::new().embed(ui::intro_embed());

    if let Err(err) = channel_id.send_message(&ctx.http, message).await {
        warn!(
            "could not send the introduction panel in guild {}: {err}",
            guild.id
        );
    }
}

async fn register_commands(ctx: &Context, guild_id: GuildId) {
    match guild_id
        .set_commands(&ctx.http, command_definitions())
        .await
    {
        Ok(_) => info!("slash commands registered for guild {guild_id}"),
        Err(err) => error!("could not register slash commands for guild {guild_id}: {err}"),
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
