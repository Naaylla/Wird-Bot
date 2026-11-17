use chrono::NaiveDate;
use serenity::all::{ButtonStyle, CreateActionRow, CreateButton, CreateEmbed, CreateEmbedFooter};

use crate::db::LeaderEntry;
use crate::quran::{self, Ayah, Chapter};

const GREEN: u32 = 0x2E_CC_71;

pub fn daily_embed(
    day: NaiveDate,
    ayah: &Ayah,
    chapter: Option<&Chapter>,
    done: bool,
) -> CreateEmbed {
    let (surah, ayah_number) = quran::verse_parts(&ayah.verse_key);
    let title = match chapter {
        Some(chapter) => format!(
            "{} · {} · {surah}:{ayah_number}",
            chapter.name, chapter.arabic
        ),
        None => format!("Surah {surah} · {surah}:{ayah_number}"),
    };
    let body = format!(
        "{}\n\n**Translation**\n{}",
        ayah.arabic,
        clean_text(&ayah.translation)
    );
    let footer = if done {
        format!("Memorized on {day}. Your next ayah is available tomorrow.")
    } else {
        "Click \"Memorized\" when you have learned this ayah. Your sequence advances the next day."
            .to_owned()
    };

    CreateEmbed::new()
        .title(title)
        .url(format!("https://quran.com/{surah}/{ayah_number}"))
        .description(truncate(&body, 4_000))
        .colour(GREEN)
        .footer(CreateEmbedFooter::new(footer))
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

pub fn progress_embed(total: u32, streak: u32, done_today: bool) -> CreateEmbed {
    let today = if done_today {
        "Today's ayah: memorized ✓"
    } else {
        "Today's ayah: not done yet"
    };

    CreateEmbed::new()
        .title("Your Wird")
        .description(format!(
            "**Ayahs memorized:** {total}\n**Current streak:** {streak} day(s)\n**{today}**"
        ))
        .colour(GREEN)
        .footer(CreateEmbedFooter::new("One ayah a day"))
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

pub fn daily_buttons(day: NaiveDate, verse_key: &str, done: bool) -> CreateActionRow {
    let day = day.format("%Y-%m-%d");

    let memorized = CreateButton::new(format!("wird|memorized|{day}|{verse_key}"))
        .label(if done { "Memorized ✓" } else { "Memorized" })
        .style(ButtonStyle::Success)
        .disabled(done);
    let tafsir_en = CreateButton::new(format!("wird|tafsir_en|{verse_key}"))
        .label("Tafsir (EN)")
        .style(ButtonStyle::Secondary);
    let tafsir_ar = CreateButton::new(format!("wird|tafsir_ar|{verse_key}"))
        .label("Tafsir (AR)")
        .style(ButtonStyle::Secondary);

    CreateActionRow::Buttons(vec![memorized, tafsir_en, tafsir_ar])
}

pub fn clean_text(input: &str) -> String {
    let without_notes = remove_tag_blocks(input, "sup");
    normalize(&strip_tags(&without_notes))
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

    normalize(&strip_tags(&with_breaks))
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
    fn translation_footnotes_are_removed() {
        assert_eq!(
            clean_text("Allah<sup foot_note=1>2</sup> is One."),
            "Allah is One."
        );
    }
}
