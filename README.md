# Wird

A Discord bot that helps you memorise **one ayah of the Qur'an per day**.

- `/wird` posts today's ayah with its Arabic text and an English translation.
- Click **Memorized** when you have memorised it. You cannot mark another ayah until the next day.
- Click **Tafsir (EN)** or **Tafsir (AR)** to read the tafsir privately.
- `/progress` shows how many ayahs you have memorised and your current streak.

Ayah text, translations and tafsir come from the public [Quran.com API](https://api.quran.com/api/v4). Your progress is stored locally in SQLite (`wird.db`), so it survives restarts.

## What you need to download

1. **Rust** using [rustup](https://rustup.rs). This gives you `cargo` and `rustc`.
2. **Visual Studio Build Tools** with the **Desktop development with C++** workload. Rust needs its linker on Windows. During rustup setup it can install this for you; otherwise install it from <https://visualstudio.microsoft.com/visual-cpp-build-tools/>.
3. **Git** (already available on most machines).
4. Optional: **GitHub CLI** (`winget install GitHub.cli`) if you want to push from the terminal.

Check that everything works:

```powershell
rustc --version
cargo --version
```

## Create your Discord bot

1. Open the [Discord Developer Portal](https://discord.com/developers/applications) and click **New Application**.
2. Go to **Bot** and click **Reset Token**. Copy the token. Never share it or commit it.
3. You do **not** need any privileged intents for this bot.
4. Go to **OAuth2 -> URL Generator**, select the scopes `bot` and `applications.commands`, and the permissions **Send Messages** and **Embed Links**. Open the generated URL to invite the bot to your server.

Invite URL template (replace `YOUR_APPLICATION_ID`):

```text
https://discord.com/api/oauth2/authorize?client_id=YOUR_APPLICATION_ID&permissions=18432&scope=bot%20applications.commands
```

## Run it

```powershell
Copy-Item .env.example .env
notepad .env
```

Put your bot token after `DISCORD_TOKEN=`, save, then run:

```powershell
cargo run
```

You should see `Wird is online` and `slash commands registered`.

**Tip:** global slash commands can take up to an hour to appear. Put your server ID in `GUILD_ID` in `.env` while developing and they appear instantly.

## Using the bot

| Command | What it does |
| --- | --- |
| `/wird` | Shows today's ayah and the Memorized / Tafsir buttons |
| `/progress` | Shows your total and your streak (private) |

The daily ayah uses **UTC dates**, so everyone sees the same ayah on the same day and it changes at midnight UTC.

## How it works

```text
main.rs        loads .env, opens SQLite, fetches surah names, starts Discord
  |
  +-- bot.rs        event handler: registers commands, routes commands/buttons
  |     +-- commands.rs     /wird and /progress
  |     +-- components.rs   Memorized and Tafsir button clicks
  |
  +-- quran.rs      Quran.com API + which ayah belongs to which date
  +-- db.rs         SQLite schema and queries
  +-- ui.rs         Discord embeds, buttons, HTML cleanup
  +-- data.rs       shared state (database, HTTP client, surah names)
```

### Choosing the ayah of the day

`daily_verse_key` in `src/quran.rs` turns the date into a number between 1 and 6236 (the number of ayahs in the Qur'an):

```text
index = (days_since_epoch * 7919) % 6236 + 1
```

The number is then mapped to a `surah:ayah` key using the length of each surah. This means:

- the same ayah for everyone on a given UTC day,
- a different ayah every day,
- every ayah is visited before the cycle repeats (about 17 years).

### Enforcing one ayah per day

The `memorizations` table has a primary key on `(user_id, day)`:

```sql
PRIMARY KEY (user_id, day)
```

The Memorized button uses `INSERT OR IGNORE`. The first click inserts a row; later clicks on the same day insert nothing. That is the whole "one per day" rule, enforced by the database rather than by timers.

### Buttons

Button IDs look like `wird|memorized|2026-11-12|2:255`, so the clicked button always refers to the exact ayah and day that was shown.

Tafsir requires a network request, and Discord expects an answer within 3 seconds. The bot defers the interaction ("thinking"), fetches the tafsir, then edits the private response into an embed. Tafsir HTML is stripped, entities are decoded, and the text is truncated to Discord's embed limit.

## Development commands

```powershell
cargo check          # fast type check
cargo test           # offline unit tests
cargo run            # start the bot
cargo build --release
```

The release binary is written to `target\release\wird.exe`.

## Notes

- Keep `.env` private. It is already in `.gitignore`.
- `wird.db` is your data. Back it up if you care about your streak.
- The bot only needs the `GUILDS` gateway intent because it uses slash commands, not message content.

## License

MIT
