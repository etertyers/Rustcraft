//! JSON text components (M44), as `/tellraw`, `/title`, bossbars and item
//! names take them: plain strings, objects with `text`, `translate`,
//! `score`, `selector` or `keybind`, a color and styles, `extra` children,
//! a `clickEvent` and a `hoverEvent`. They are turned into the game's `§`
//! coded strings; the click and hover of a line are kept beside it.

use serde_json::Value;

/// What a component needs from the game to be written out.
pub struct TextContext<'a> {
    /// A holder's score in an objective.
    pub score: &'a dyn Fn(&str, &str) -> Option<i32>,
    /// The names a selector picks.
    pub selector: &'a dyn Fn(&str) -> Result<Vec<String>, String>,
}

/// A component written out: its `§` text, and the first click and hover.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Rendered {
    pub text: String,
    pub click: Option<(String, String)>,
    pub hover: Option<String>,
}

#[derive(Debug, Clone, Default)]
struct Style {
    color: Option<char>,
    bold: bool,
    italic: bool,
    underlined: bool,
    strikethrough: bool,
    obfuscated: bool,
}

impl Style {
    fn codes(&self) -> String {
        let mut out = String::from("§r");
        if let Some(c) = self.color {
            out.push('§');
            out.push(c);
        }
        for (on, code) in [
            (self.obfuscated, 'k'),
            (self.bold, 'l'),
            (self.strikethrough, 'm'),
            (self.underlined, 'n'),
            (self.italic, 'o'),
        ] {
            if on {
                out.push('§');
                out.push(code);
            }
        }
        out
    }
}

/// The `§` code letter nearest to a `#rrggbb` color.
fn nearest(hex: &str) -> Option<char> {
    let n = u32::from_str_radix(hex.strip_prefix('#')?, 16).ok()?;
    let (r, g, b) = (
        i64::from((n >> 16) & 255),
        i64::from((n >> 8) & 255),
        i64::from(n & 255),
    );
    const PALETTE: [(char, (i64, i64, i64)); 16] = [
        ('0', (0, 0, 0)),
        ('1', (0, 0, 170)),
        ('2', (0, 170, 0)),
        ('3', (0, 170, 170)),
        ('4', (170, 0, 0)),
        ('5', (170, 0, 170)),
        ('6', (255, 170, 0)),
        ('7', (170, 170, 170)),
        ('8', (85, 85, 85)),
        ('9', (85, 85, 255)),
        ('a', (85, 255, 85)),
        ('b', (85, 255, 255)),
        ('c', (255, 85, 85)),
        ('d', (255, 85, 255)),
        ('e', (255, 255, 85)),
        ('f', (255, 255, 255)),
    ];
    PALETTE
        .iter()
        .min_by_key(|(_, (pr, pg, pb))| (pr - r).pow(2) + (pg - g).pow(2) + (pb - b).pow(2))
        .map(|(c, _)| *c)
}

/// A few of vanilla's English strings; any other key shows as itself.
fn translation(key: &str) -> Option<&'static str> {
    Some(match key {
        "chat.type.text" => "<%s> %s",
        "chat.type.announcement" => "[%s] %s",
        "chat.type.emote" => "* %s %s",
        "multiplayer.player.joined" => "%s joined the game",
        "multiplayer.player.left" => "%s left the game",
        "death.attack.generic" => "%s died",
        "death.attack.player" => "%s was slain by %s",
        "death.attack.mob" => "%s was slain by %s",
        "death.attack.fall" => "%s hit the ground too hard",
        "death.attack.drown" => "%s drowned",
        "death.attack.lava" => "%s tried to swim in lava",
        "gameMode.survival" => "Survival Mode",
        "gameMode.creative" => "Creative Mode",
        "gameMode.adventure" => "Adventure Mode",
        "gameMode.spectator" => "Spectator Mode",
        "options.difficulty.peaceful" => "Peaceful",
        "options.difficulty.easy" => "Easy",
        "options.difficulty.normal" => "Normal",
        "options.difficulty.hard" => "Hard",
        _ => return None,
    })
}

/// Writes `json` out as `§` text. A malformed component is an error with
/// vanilla's wording.
pub fn render(json: &str, ctx: &TextContext) -> Result<Rendered, String> {
    let value: Value = if json.trim_start().starts_with(['{', '[', '"']) {
        serde_json::from_str(json).map_err(|e| format!("Invalid chat component: {e}"))?
    } else if let Ok(number) = json.trim().parse::<f64>() {
        Value::from(number)
    } else {
        // Bare words are what a player meant as a plain message.
        Value::String(json.to_owned())
    };
    let mut out = Rendered::default();
    write(&value, &Style::default(), ctx, &mut out)?;
    Ok(out)
}

fn write(
    value: &Value,
    inherited: &Style,
    ctx: &TextContext,
    out: &mut Rendered,
) -> Result<(), String> {
    match value {
        Value::String(text) => {
            push(out, inherited, text);
            Ok(())
        }
        Value::Number(n) => {
            push(out, inherited, &n.to_string());
            Ok(())
        }
        Value::Bool(b) => {
            push(out, inherited, &b.to_string());
            Ok(())
        }
        Value::Array(items) => {
            // The first element's style is inherited by the rest.
            let first = items.first().and_then(|v| style_of(v, inherited).ok());
            let shared = first.unwrap_or_else(|| inherited.clone());
            for (i, item) in items.iter().enumerate() {
                write(item, if i == 0 { inherited } else { &shared }, ctx, out)?;
            }
            Ok(())
        }
        Value::Object(map) => {
            let style = style_of(value, inherited)?;
            if let Some(Value::Object(click)) = map.get("clickEvent").or(map.get("click_event")) {
                let action = click.get("action").and_then(Value::as_str);
                let value = click
                    .get("value")
                    .or(click.get("command"))
                    .or(click.get("url"))
                    .and_then(Value::as_str);
                if let (Some(a), Some(v)) = (action, value) {
                    const ACTIONS: &[&str] = &[
                        "run_command",
                        "suggest_command",
                        "open_url",
                        "copy_to_clipboard",
                        "change_page",
                    ];
                    if !ACTIONS.contains(&a) {
                        return Err(format!("Don't know how to turn {a} into a click action"));
                    }
                    out.click.get_or_insert((a.to_owned(), v.to_owned()));
                }
            }
            if let Some(Value::Object(hover)) = map.get("hoverEvent").or(map.get("hover_event")) {
                let action = hover.get("action").and_then(Value::as_str);
                let contents = hover.get("contents").or(hover.get("value"));
                if action == Some("show_text")
                    && let Some(contents) = contents
                {
                    let mut text = Rendered::default();
                    write(contents, &Style::default(), ctx, &mut text)?;
                    out.hover.get_or_insert(text.text);
                }
            }
            if let Some(text) = map.get("text") {
                let shown = match text {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                push(out, &style, &shown);
            } else if let Some(Value::String(key)) = map.get("translate") {
                let args: Vec<String> = match map.get("with") {
                    Some(Value::Array(items)) => items
                        .iter()
                        .map(|item| {
                            let mut part = Rendered::default();
                            write(item, &Style::default(), ctx, &mut part).map(|()| {
                                // An argument unstyled adds no reset of its own.
                                part.text
                                    .strip_prefix("§r")
                                    .unwrap_or(&part.text)
                                    .to_owned()
                            })
                        })
                        .collect::<Result<_, _>>()?,
                    _ => Vec::new(),
                };
                let template: String = translation(key).map_or_else(
                    || {
                        map.get("fallback")
                            .and_then(Value::as_str)
                            .unwrap_or(key)
                            .to_owned()
                    },
                    str::to_owned,
                );
                push(out, &style, &fill(&template, &args));
            } else if let Some(Value::Object(score)) = map.get("score") {
                let name = score.get("name").and_then(Value::as_str).unwrap_or("");
                let objective = score.get("objective").and_then(Value::as_str).unwrap_or("");
                let holders = if name.starts_with('@') {
                    (ctx.selector)(name)?
                } else {
                    vec![name.to_owned()]
                };
                let value = holders
                    .first()
                    .and_then(|h| (ctx.score)(h, objective))
                    .map_or_else(String::new, |v| v.to_string());
                push(out, &style, &value);
            } else if let Some(Value::String(selector)) = map.get("selector") {
                let names = (ctx.selector)(selector)?;
                push(out, &style, &names.join(", "));
            } else if let Some(Value::String(key)) = map.get("keybind") {
                let named = crate::bindings::Action::from_id(key)
                    .map(|a| crate::bindings::input_name(a.default_input()))
                    .unwrap_or_else(|| key.clone());
                push(out, &style, &named);
            } else if map.contains_key("nbt") {
                return Err("Text components can't read data (nbt) in this game".to_owned());
            }
            if let Some(Value::Array(extra)) = map.get("extra") {
                for child in extra {
                    write(child, &style, ctx, out)?;
                }
            }
            Ok(())
        }
        Value::Null => Err("Invalid chat component: null".to_owned()),
    }
}

/// The style a component sets on top of `inherited`.
fn style_of(value: &Value, inherited: &Style) -> Result<Style, String> {
    let mut style = inherited.clone();
    let Value::Object(map) = value else {
        return Ok(style);
    };
    if let Some(Value::String(color)) = map.get("color") {
        style.color = if color == "reset" {
            None
        } else if color.starts_with('#') {
            nearest(color)
        } else {
            Some(
                super::board::color_code(color)
                    .ok_or_else(|| format!("Invalid color name: {color}"))?,
            )
        };
    }
    for (key, field) in [
        ("bold", &mut style.bold),
        ("italic", &mut style.italic),
        ("underlined", &mut style.underlined),
        ("strikethrough", &mut style.strikethrough),
        ("obfuscated", &mut style.obfuscated),
    ] {
        if let Some(Value::Bool(on)) = map.get(key) {
            *field = *on;
        }
    }
    Ok(style)
}

fn push(out: &mut Rendered, style: &Style, text: &str) {
    out.text.push_str(&style.codes());
    out.text.push_str(text);
}

/// Fills `%s` and `%1$s` in a translation.
fn fill(template: &str, args: &[String]) -> String {
    let mut out = String::new();
    let mut next = 0;
    let mut chars = template.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        let mut digits = String::new();
        while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
            digits.push(d);
            chars.next();
        }
        match chars.next() {
            Some('%') if digits.is_empty() => out.push('%'),
            Some('$') if chars.peek() == Some(&'s') => {
                chars.next();
                let at: usize = digits.parse().unwrap_or(1);
                out.push_str(args.get(at.saturating_sub(1)).map_or("", String::as_str));
            }
            Some('s') => {
                out.push_str(args.get(next).map_or("", String::as_str));
                next += 1;
            }
            Some(other) => {
                out.push('%');
                out.push_str(&digits);
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx() -> TextContext<'static> {
        TextContext {
            score: &|holder, objective| (holder == "Kenzo" && objective == "kills").then_some(7),
            selector: &|s| {
                if s == "@a" {
                    Ok(vec!["Kenzo".to_owned(), "Alex".to_owned()])
                } else {
                    Err("No entity was found".to_owned())
                }
            },
        }
    }

    fn text(json: &str) -> String {
        render(json, &ctx()).unwrap().text
    }

    #[test]
    fn strings_objects_and_arrays_become_coded_text() {
        assert_eq!(text(r#""hello""#), "§rhello");
        assert_eq!(text("hello world"), "§rhello world");
        assert_eq!(
            text(r##"{"text":"Hi","color":"red","bold":true}"##),
            "§r§c§lHi"
        );
        assert_eq!(
            text(
                r##"{"text":"a","color":"gold","extra":[{"text":"b"},{"text":"c","color":"white"}]}"##
            ),
            "§r§6a§r§6b§r§fc"
        );
        assert_eq!(text(r##"["x",{"text":"y"}]"##), "§rx§ry",);
        assert_eq!(text(r##"{"text":"#","color":"#ff0000"}"##), "§r§4#");
    }

    #[test]
    fn scores_selectors_translations_and_keybinds_resolve() {
        assert_eq!(
            text(r#"{"score":{"name":"Kenzo","objective":"kills"}}"#),
            "§r7"
        );
        assert_eq!(
            text(r#"{"score":{"name":"Alex","objective":"kills"}}"#),
            "§r"
        );
        assert_eq!(text(r#"{"selector":"@a"}"#), "§rKenzo, Alex");
        assert_eq!(
            text(r#"{"translate":"chat.type.text","with":["Kenzo",{"text":"hey"}]}"#),
            "§r<Kenzo> hey"
        );
        assert_eq!(text(r#"{"translate":"nothing.here"}"#), "§rnothing.here");
        assert_eq!(
            text(r#"{"translate":"x.y","fallback":"%s of %2$s","with":["a","b"]}"#),
            "§ra of b"
        );
        assert_eq!(text(r#"{"keybind":"key.jump"}"#), "§rSpace");
    }

    #[test]
    fn click_and_hover_are_kept_and_bad_components_are_errors() {
        let r = render(
            r##"{"text":"go","clickEvent":{"action":"run_command","value":"/time set day"},"hoverEvent":{"action":"show_text","contents":{"text":"Sunrise","color":"yellow"}}}"##,
            &ctx(),
        )
        .unwrap();
        assert_eq!(
            r.click,
            Some(("run_command".into(), "/time set day".into()))
        );
        assert_eq!(r.hover.as_deref(), Some("§r§eSunrise"));
        assert!(
            render(
                r#"{"text":"x","clickEvent":{"action":"explode","value":"1"}}"#,
                &ctx()
            )
            .is_err()
        );
        assert!(render(r#"{"text":"x","color":"mauve"}"#, &ctx()).is_err());
        assert!(render(r#"{"text":"#, &ctx()).is_err());
        assert!(render(r#"{"selector":"@e"}"#, &ctx()).is_err());
    }
}
