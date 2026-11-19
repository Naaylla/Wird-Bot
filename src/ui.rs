use chrono::NaiveDate;
use serenity::all::{
    ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter, CreateSelectMenu,
    CreateSelectMenuKind, CreateSelectMenuOption,
};

use crate::chapters::{Chapter, ChapterList};
use crate::db::LeaderEntry;
use crate::quran::{self, Ayah};

const GREEN: u32 = 0x2E_CC_71;
const SURAHS_PER_PAGE: usize = 25;
const BASMALA: &str = "بِسْمِ اللَّهِ الرَّحْمَٰنِ الرَّحِيمِ";

pub fn daily_embeds(
    ayahs: &[Ayah],
    chapters: &ChapterList,
    completed: u16,
    target: u16,
    plan_complete: bool,
) -> Vec<CreateEmbed> {
    let remaining = target.saturating_sub(completed);
    let footer = if plan_complete {
        format!("This plan is complete · {completed}/{target} ayahs memorized today.")
    } else if remaining == 0 {
        format!("Today's wird is complete: {completed}/{target} ayahs.")
    } else {
        format!("{completed}/{target} ayahs memorized today · {remaining} ayah(s) left.")
    };

    let mut descriptions = Vec::new();
    let mut description = String::new();
    for ayah in ayahs {
        let body = format!(
            "**{}**\n{}\n\n**Translation**\n{}\n\n",
            ayah.verse_key,
            daily_arabic(ayah),
            clean_text(&ayah.translation)
        );
        if !description.is_empty() && description.chars().count() + body.chars().count() > 3_900 {
            descriptions.push(std::mem::take(&mut description));
        }
        description.push_str(&truncate(&body, 3_900));
    }
    if !description.is_empty() {
        descriptions.push(description);
    }

    let (first_surah, first_ayah) = ayahs
        .first()
        .map(|ayah| quran::verse_parts(&ayah.verse_key))
        .unwrap_or((1, 1));
    descriptions
        .into_iter()
        .enumerate()
        .map(|(index, description)| {
            let title = if ayahs.len() == 1 {
                chapters
                    .get(&first_surah)
                    .map(|chapter| format!("{} · {}", chapter.name, ayahs[0].verse_key))
                    .unwrap_or_else(|| format!("Surah {first_surah} · {}", ayahs[0].verse_key))
            } else if index == 0 {
                format!("Today's Wird · {} ayahs", ayahs.len())
            } else {
                format!("Today's Wird · continued ({}/{})", index + 1, ayahs.len())
            };
            CreateEmbed::new()
                .title(title)
                .url(format!("https://quran.com/{first_surah}/{first_ayah}"))
                .description(description)
                .colour(GREEN)
                .footer(CreateEmbedFooter::new(footer.clone()))
        })
        .collect()
}

pub fn tafsir_embed(
    verse_key: &str,
    chapter: Option<&Chapter>,
    source: &str,
    text: &str,
) -> CreateEmbed {
    let (surah, ayah_number) = quran::verse_parts(verse_key);
    let title = match chapter {
        Some(chapter) => format!("Tafsir · {} {surah}:{ayah_number}", chapter.name),
        None => format!("Tafsir · {surah}:{ayah_number}"),
    };

    CreateEmbed::new()
        .title(title)
        .url(format!("https://quran.com/{surah}/{ayah_number}"))
        .description(truncate(&clean_tafsir(text), 4_000))
        .colour(GREEN)
        .footer(CreateEmbedFooter::new(format!("Source: {source}")))
}

fn daily_arabic(ayah: &Ayah) -> String {
    let (surah, ayah_number) = quran::verse_parts(&ayah.verse_key);
    if ayah_number == 1 && surah != 1 && surah != 9 {
        format!("{BASMALA}\n\n{}", ayah.arabic)
    } else {
        ayah.arabic.clone()
    }
}

pub fn progress_embed(total: u32, streak: u32, done_today: bool) -> CreateEmbed {
    let today = if done_today {
        "Today's wird: complete ✓"
    } else {
        "Today's wird: not complete yet"
    };

    CreateEmbed::new()
        .title("Your Wird")
        .description(format!(
            "**Ayahs memorized:** {total}\n**Current streak:** {streak} day(s)\n**{today}**"
        ))
        .colour(GREEN)
        .footer(CreateEmbedFooter::new(
            "Your daily goal, one ayah at a time",
        ))
}

pub fn help_embed() -> CreateEmbed {
    CreateEmbed::new()
        .title("Wird — how it works")
        .description(
            "**Your daily goal, one ayah at a time.**\n\n\
             1. Choose a plan with `/plan` — select the full Qur'an or one surah, then choose 1–10 ayahs per day.\n\
             2. Run `/wird` to see all of today's ayahs together.\n\
             3. Tap **Memorized** for each ayah once you have learned it.\n\n\
             **Commands**\n\
             `/plan` — choose a plan and daily size in separate steps\n\
             `/wird` — show today's ayahs together and mark each one memorized\n\
             `/progress` — see your total and daily streak\n\
             `/leaderboard` — the server's streak leaderboard\n\
             `/remind` — get a daily DM when your wird is unfinished\n\
             `/reset` — reset your progress in this server\n\
             `/help` — this message",
        )
        .colour(GREEN)
        .footer(CreateEmbedFooter::new("Your daily goal, one ayah at a time"))
}

pub fn intro_embed() -> CreateEmbed {
    CreateEmbed::new()
        .title("Welcome to Wird")
        .description(
            "**Assalamu alaikum!** I help your server memorize the Qur'an together — one ayah at a time, at your own pace.\n\n\
             **How it works**\n\
             1. `/plan` — choose the full Qur'an or a surah, then set 1–10 ayahs per day.\n\
             2. `/wird` — see today's ayahs together with their Arabic text and translation.\n\
             3. Tap **Memorized** on each ayah when you have learned it. Come back daily to grow your streak.\n\n\
             **Commands**\n\
             `/plan` · `/wird` · `/progress` · `/leaderboard` · `/remind` · `/reset` · `/help`\n\n\
             Start with `/plan`, and may Allah make it easy for you.",
        )
        .colour(GREEN)
        .footer(CreateEmbedFooter::new("One ayah at a time · /help for everything"))
}

pub fn intro_buttons() -> CreateActionRow {
    CreateActionRow::Buttons(vec![CreateButton::new("wird|help")
        .label("How it works")
        .style(ButtonStyle::Secondary)])
}

pub fn leaderboard_embed(entries: &[LeaderEntry], today: NaiveDate) -> CreateEmbed {
    let description = if entries.is_empty() {
        "No one has memorized an ayah yet. Be the first — run `/wird`.".to_owned()
    } else {
        entries
            .iter()
            .enumerate()
            .map(|(index, entry)| {
                format!(
                    "**#{}** <@{}> — {} · {}",
                    index + 1,
                    entry.user_id,
                    plural(entry.streak, "day"),
                    plural(entry.total, "ayah"),
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    };

    CreateEmbed::new()
        .title("Wird Leaderboard")
        .description(description)
        .colour(GREEN)
        .footer(CreateEmbedFooter::new(format!(
            "Daily memorization streaks · {today}"
        )))
}

fn plural(count: u32, word: &str) -> String {
    if count == 1 {
        format!("{count} {word}")
    } else {
        format!("{count} {word}s")
    }
}

pub fn surah_picker(page: usize, chapters: &ChapterList) -> (String, Vec<CreateActionRow>) {
    let pages = surah_page_count(chapters).max(1);
    let page = page.min(pages - 1);
    let (start, end) = surah_page_bounds(page, chapters.len());

    let options = chapters
        .iter()
        .skip(start)
        .take(end - start)
        .map(|(surah, chapter)| {
            CreateSelectMenuOption::new(format!("{surah}. {}", chapter.name), surah.to_string())
                .description(format!("{} · {} ayahs", chapter.arabic, chapter.verses))
        })
        .collect();

    let menu = CreateSelectMenu::new(
        "wird|surah_select",
        CreateSelectMenuKind::String { options },
    )
    .placeholder("Choose a surah");

    let previous = CreateButton::new(surah_page_id(page.saturating_sub(1)))
        .label("Previous")
        .style(ButtonStyle::Secondary)
        .disabled(page == 0);
    let next = CreateButton::new(surah_page_id(page + 1))
        .label("Next")
        .style(ButtonStyle::Secondary)
        .disabled(page + 1 >= pages);

    let content = format!(
        "Step 2 of 3: choose the surah you want to memorize. Page {}/{} · {} surahs.",
        page + 1,
        pages,
        chapters.len()
    );

    (
        content,
        vec![
            CreateActionRow::SelectMenu(menu),
            CreateActionRow::Buttons(vec![previous, next]),
        ],
    )
}

fn surah_page_id(page: usize) -> String {
    format!("wird|surah_page|{page}")
}

pub fn daily_picker(target_surah: Option<u16>) -> (String, Vec<CreateActionRow>) {
    let options = (1..=10)
        .map(|daily| {
            CreateSelectMenuOption::new(
                format!("{daily} ayah{}", if daily == 1 { "" } else { "s" }),
                daily.to_string(),
            )
        })
        .collect();
    let menu = CreateSelectMenu::new(
        daily_target_id(target_surah),
        CreateSelectMenuKind::String { options },
    )
    .placeholder("Choose your daily number of ayahs");
    (
        format!(
            "Step {} of {}: choose how many ayahs to memorize each day.",
            if target_surah.is_some() { 3 } else { 2 },
            if target_surah.is_some() { 3 } else { 2 }
        ),
        vec![CreateActionRow::SelectMenu(menu)],
    )
}

fn daily_target_id(target_surah: Option<u16>) -> String {
    let target = target_surah
        .map(|surah| surah.to_string())
        .unwrap_or_else(|| "quran".to_owned());
    format!("wird|daily_select|{target}")
}

fn surah_page_count(chapters: &ChapterList) -> usize {
    chapters.len().div_ceil(SURAHS_PER_PAGE)
}

fn surah_page_bounds(page: usize, total: usize) -> (usize, usize) {
    let start = page * SURAHS_PER_PAGE;
    (start, (start + SURAHS_PER_PAGE).min(total))
}

pub fn daily_buttons(
    day: NaiveDate,
    ayahs: &[Ayah],
    completed_today: &[String],
) -> Vec<CreateActionRow> {
    let day = day.format("%Y-%m-%d");
    let buttons = ayahs
        .iter()
        .map(|ayah| {
            let done = completed_today.contains(&ayah.verse_key);
            CreateButton::new(format!("wird|memorized|{day}|{}", ayah.verse_key))
                .label(if done {
                    format!("{} ✓", ayah.verse_key)
                } else {
                    format!("Memorized {}", ayah.verse_key)
                })
                .style(ButtonStyle::Success)
                .disabled(done)
        })
        .collect::<Vec<_>>();
    let button_rows = buttons
        .chunks(5)
        .map(|buttons| CreateActionRow::Buttons(buttons.to_vec()))
        .collect::<Vec<_>>();
    let options = ayahs
        .iter()
        .map(|ayah| CreateSelectMenuOption::new(ayah.verse_key.clone(), ayah.verse_key.clone()))
        .collect();
    let tafsir_menu = CreateSelectMenu::new(
        "wird|tafsir_select",
        CreateSelectMenuKind::String { options },
    )
    .placeholder("Choose an ayah for tafsir");
    let mut rows = button_rows;
    rows.push(CreateActionRow::SelectMenu(tafsir_menu));
    rows
}

pub fn tafsir_buttons(verse_key: &str) -> CreateActionRow {
    CreateActionRow::Buttons(vec![
        CreateButton::new(format!("wird|tafsir_en|{verse_key}"))
            .label("Tafsir (EN)")
            .style(ButtonStyle::Secondary),
        CreateButton::new(format!("wird|tafsir_ar|{verse_key}"))
            .label("Tafsir (AR)")
            .style(ButtonStyle::Secondary),
    ])
}

pub fn reset_prompt_buttons() -> CreateActionRow {
    CreateActionRow::Buttons(vec![
        CreateButton::new("wird|reset|confirm_prompt")
            .label("Continue")
            .style(ButtonStyle::Danger),
        CreateButton::new("wird|reset|cancel")
            .label("Cancel")
            .style(ButtonStyle::Secondary),
    ])
}

pub fn reset_confirm_buttons() -> CreateActionRow {
    CreateActionRow::Buttons(vec![
        CreateButton::new("wird|reset|confirm")
            .label("Yes, delete everything")
            .style(ButtonStyle::Danger),
        CreateButton::new("wird|reset|cancel")
            .label("Cancel")
            .style(ButtonStyle::Secondary),
    ])
}

pub fn memorization_announcement(user_id: u64, verse_key: &str) -> String {
    format!(
        "MashaAllah! <@{user_id}> just memorized **{verse_key}** today. What about you? Go for it, one ayah at a time, in shaa Allah!"
    )
}

pub fn reset_cancel_button() -> CreateActionRow {
    CreateActionRow::Buttons(vec![CreateButton::new("wird|reset|dismiss")
        .label("Cancel")
        .style(ButtonStyle::Secondary)])
}

pub fn clean_text(input: &str) -> String {
    let without_notes = remove_tag_blocks(input, "sup");
    escape_markdown(&normalize(&strip_tags(&without_notes)))
}

pub fn clean_tafsir(input: &str) -> String {
    let without_notes = remove_tag_blocks(input, "sup");
    let with_breaks = without_notes
        .replace("<p>", "\n\n")
        .replace("</p>", "\n")
        .replace("<h2>", "\n\n")
        .replace("</h2>", "\n")
        .replace("<h3>", "\n\n")
        .replace("</h3>", "\n")
        .replace("<br>", "\n")
        .replace("<br/>", "\n")
        .replace("<br />", "\n");

    escape_markdown(&normalize(&strip_tags(&with_breaks)))
}

fn escape_markdown(input: &str) -> String {
    let mut output = String::with_capacity(input.len());

    for character in input.chars() {
        match character {
            '`' => output.push('\''),
            '\\' | '*' | '_' | '~' | '>' | '#' | '[' | ']' => {
                output.push('\\');
                output.push(character);
            }
            _ => output.push(character),
        }
    }

    output
}

fn remove_tag_blocks(input: &str, tag: &str) -> String {
    let open = format!("<{tag}");
    let close = format!("</{tag}>");
    let mut output = String::with_capacity(input.len());
    let mut rest = input;

    while let Some(start) = rest.find(&open) {
        output.push_str(&rest[..start]);
        match rest[start..].find(&close) {
            Some(end) => rest = &rest[start + end + close.len()..],
            None => {
                rest = "";
                break;
            }
        }
    }

    output.push_str(rest);
    output
}

fn strip_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut inside_tag = false;

    for character in input.chars() {
        match character {
            '<' => inside_tag = true,
            '>' => inside_tag = false,
            _ if !inside_tag => output.push(character),
            _ => {}
        }
    }

    html_escape::decode_html_entities(&output).into_owned()
}

fn normalize(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut newlines = 0u8;
    let mut last_was_space = false;

    for character in input.chars() {
        if character == '\n' {
            newlines += 1;
            if newlines <= 2 {
                output.push('\n');
            }
            last_was_space = false;
        } else if character.is_whitespace() {
            if !last_was_space && !output.ends_with('\n') {
                output.push(' ');
            }
            last_was_space = true;
            newlines = 0;
        } else {
            output.push(character);
            last_was_space = false;
            newlines = 0;
        }
    }

    output.trim().to_owned()
}

fn truncate(text: &str, limit: usize) -> String {
    let mut characters = text.chars();
    let mut output: String = characters.by_ref().take(limit).collect();

    if characters.next().is_some() {
        output.pop();
        output.push('…');
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tafsir_html_becomes_readable_text() {
        let raw = "<h2>Title</h2><p>Hello&nbsp;<b>world</b>.</p><sup foot_note=1>2</sup>";
        assert_eq!(clean_tafsir(raw), "Title\n\nHello world.");
    }

    #[test]
    fn tafsir_backticks_do_not_become_code_blocks() {
        assert_eq!(
            clean_tafsir("<p>Imam Ahmad recorded that `A'ishah said, \"Hi.\"</p>"),
            "Imam Ahmad recorded that 'A'ishah said, \"Hi.\""
        );
    }

    #[test]
    fn translation_footnotes_are_removed() {
        assert_eq!(
            clean_text("Allah<sup foot_note=1>2</sup> is One."),
            "Allah is One."
        );
    }

    #[test]
    fn memorization_announcement_mentions_only_the_completer() {
        let message = memorization_announcement(123, "2:255");

        assert!(message.contains("<@123>"));
        assert!(message.contains("**2:255**"));
        assert_eq!(message.matches("<@").count(), 1);
    }

    #[test]
    fn surah_pages_cover_every_surah_once() {
        let chapters: ChapterList = (1..=114)
            .map(|surah| {
                (
                    surah,
                    Chapter {
                        name: format!("Surah {surah}"),
                        arabic: String::new(),
                        verses: 3,
                    },
                )
            })
            .collect();

        assert_eq!(surah_page_count(&chapters), 5);
        assert_eq!(surah_page_bounds(0, chapters.len()), (0, 25));
        assert_eq!(surah_page_bounds(4, chapters.len()), (100, 114));
        assert_eq!(surah_page_count(&ChapterList::new()), 0);
    }

    #[test]
    fn the_surah_picker_uses_paged_navigation() {
        assert_eq!(surah_page_id(2), "wird|surah_page|2");
    }

    #[test]
    fn daily_picker_has_a_distinct_selection_step() {
        let (content, rows) = daily_picker(Some(18));
        assert!(content.contains("Step 3 of 3"));
        assert_eq!(rows.len(), 1);
        assert_eq!(daily_target_id(Some(18)), "wird|daily_select|18");
        assert_eq!(daily_target_id(None), "wird|daily_select|quran");
    }

    #[test]
    fn basmala_is_added_to_surah_openings_when_not_an_ayah() {
        let ayah = Ayah {
            verse_key: "10:1".to_owned(),
            arabic: "الر تِلْكَ آيَاتُ الْكِتَابِ الْحَكِيمِ".to_owned(),
            translation: String::new(),
        };
        assert!(daily_arabic(&ayah).starts_with(BASMALA));

        for verse_key in ["1:1", "9:1", "10:2"] {
            let ayah = Ayah {
                verse_key: verse_key.to_owned(),
                arabic: "ayah".to_owned(),
                translation: String::new(),
            };
            assert_eq!(daily_arabic(&ayah), "ayah");
        }
    }
}
