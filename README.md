# Wird

## [ADD WIRD TO YOUR DISCORD SERVER](https://discord.com/oauth2/authorize?client_id=1572037133248761926&permissions=2147535872&integration_type=0&scope=applications.commands+bot)

A Discord bot for memorising the Qur'an at your own pace, with a daily goal of 1–10 ayahs.

![Wird showing an ayah and its tafsir in Discord](assets/wird-preview.png)

## Features

- Choose the full Qur'an or any surah, with a daily goal of 1–10 ayahs.
- Browse all 114 surahs and memorize ayahs one at a time.
- Read Arabic text, English translations, and tafsir.
- Track progress and streaks, view the server leaderboard, and set daily reminders.

## Setup

1. Install [Rust](https://rustup.rs). On Windows, you may also need [Visual Studio Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) with the **Desktop development with C++** workload.
2. Create a bot in the [Discord Developer Portal](https://discord.com/developers/applications), then authorize it using the invite link above. It needs **View Channels**, **Send Messages**, and **Embed Links** permissions.
3. Copy `.env.example` to `.env` and add your bot token:

   ```powershell
   Copy-Item .env.example .env
   notepad .env
   ```

   Keep `.env` private. Slash commands are registered per server automatically.
4. Start the bot:

   ```powershell
   cargo run
   ```

## Commands

| Command | Description |
| --- | --- |
| `/plan` | Choose a whole-Qur'an or single-surah plan, then your daily size (1-10 ayahs) |
| `/wird` | Show your next ayah and mark it memorized |
| `/progress` | View your memorization total and streak |
| `/leaderboard` | View the server leaderboard |
| `/remind` | Get a daily DM when your wird is unfinished; `/remind disable:true` turns it off |
| `/timezone` | Optional override of the bot's default timezone |
| `/reset` | Reset your own progress and streak in this server |
| `/help` | A quick guide to the bot and its commands |

Reminder DMs require the person to allow direct messages from server members. Run `cargo test` to run the tests. Memorization data is stored in `wird.db`; keep that file on persistent storage when hosting the bot.

## Timezone

Discord does not send a user's timezone, so the bot uses one default for everyone. Set it once with the `DEFAULT_OFFSET` environment variable, for example `DEFAULT_OFFSET=+01:00` or `DEFAULT_OFFSET=+05:30`. Each day's wird unlocks at midnight in that timezone. When today's goal is done, Wird shows a live countdown to the next one (Discord's "in X hours"). Individuals can still override the default with `/timezone`.

## Hosting on Render

Render's filesystem is wiped on every deploy unless you mount a [persistent disk](https://render.com/docs/disks). Either use the included `render.yaml` blueprint (disk at `/var/data`), or set these env vars yourself:

```text
DATABASE_PATH=/var/data/wird.db
DEFAULT_OFFSET=+01:00
```

Without `DATABASE_PATH` on a disk, progress resets on every redeploy. Set `OWNER_ID` to your Discord user ID to enable the owner-only `/usage` command.
