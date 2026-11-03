//! Item components in command arguments (M43): `diamond_sword[damage=5,
//! enchantments={levels:{"minecraft:sharpness":3}},can_break=["stone"]]`.
//! The game has these components: `damage`, `custom_name`, `enchantments`,
//! `can_break` and `can_place_on`; any other is an error.

use super::*;

/// The component names Tab offers inside `item[...]`.
pub(super) const NAMES: &[&str] = &[
    "can_break",
    "can_place_on",
    "custom_name",
    "damage",
    "enchantments",
];

/// What Tab offers once a word is inside `item[`: the components not given yet.
pub(super) fn completions(word: &str) -> Vec<String> {
    let Some((name, rest)) = word.split_once('[') else {
        return Vec::new();
    };
    if item_named(name).is_err() {
        return Vec::new();
    }
    let (done, current) = match rest.rfind(',') {
        Some(at) => (&rest[..=at], &rest[at + 1..]),
        None => ("", rest),
    };
    if current.contains('=') {
        return Vec::new();
    }
    NAMES
        .iter()
        .filter(|n| !done.contains(&format!("{n}=")))
        .map(|n| format!("{name}[{done}{n}="))
        .collect()
}

/// Splits at the commas that are outside brackets, braces and quotes.
pub(super) fn split_top(text: &str) -> Vec<&str> {
    let (mut parts, mut depth, mut start) = (Vec::new(), 0i32, 0);
    let mut quote: Option<char> = None;
    for (i, c) in text.char_indices() {
        match (quote, c) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), _) => {}
            (None, '"' | '\'') => quote = Some(c),
            (None, '[' | '{') => depth += 1,
            (None, ']' | '}') => depth -= 1,
            (None, ',') if depth == 0 => {
                parts.push(&text[start..i]);
                start = i + 1;
            }
            _ => {}
        }
    }
    parts.push(&text[start..]);
    parts
}

/// The words in a list value: `["a","b"]`, `{blocks:["a"]}`, `{blocks:"a"}` or `"a"`.
pub(super) fn strings_in(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quote: Option<char> = None;
    for c in value.chars() {
        match (quote, c) {
            (Some(q), c) if c == q => {
                quote = None;
                out.push(std::mem::take(&mut current));
            }
            (Some(_), c) => current.push(c),
            (None, '"' | '\'') => quote = Some(c),
            _ => {}
        }
    }
    if out.is_empty() {
        // Bare words: `[stone,dirt]`.
        let inner = value.trim_matches(['[', ']', '{', '}']);
        let inner = inner.strip_prefix("blocks:").unwrap_or(inner);
        out = inner
            .trim_matches(['[', ']'])
            .split(',')
            .map(|s| s.trim().to_owned())
            .filter(|s| !s.is_empty())
            .collect();
    }
    out
}

/// A text value's plain words: `'"Excalibur"'`, `"Excalibur"` or `{"text":"Excalibur"}`.
pub(super) fn plain_text(value: &str) -> String {
    let words = strings_in(value);
    match words.as_slice() {
        [only] => only.trim_matches('"').to_owned(),
        [key, text, ..] if key == "text" => text.clone(),
        _ => value.trim_matches(['"', '\'']).to_owned(),
    }
}

fn enchantment_named(name: &str) -> Result<Enchantment, String> {
    let name = name.strip_prefix("minecraft:").unwrap_or(name);
    enchant::ALL
        .iter()
        .copied()
        .find(|e| enchantment_id(*e) == name)
        .ok_or_else(|| format!("Unknown enchantment: {name}"))
}

/// Applies `[components]` to `stack`.
pub(super) fn apply(stack: &mut ItemStack, components: &str) -> Result<(), String> {
    let mut can_break = Vec::new();
    let mut can_place_on = Vec::new();
    for part in split_top(components) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let (key, value) = part
            .split_once('=')
            .ok_or_else(|| format!("Expected '=' after component '{part}'"))?;
        let value = value.trim();
        match key.trim().strip_prefix("minecraft:").unwrap_or(key.trim()) {
            "damage" => {
                stack.damage = value
                    .parse()
                    .map_err(|_| format!("Invalid integer '{value}'"))?;
            }
            "custom_name" | "item_name" => stack.set_custom_name(Some(plain_text(value))),
            "can_break" => can_break = strings_in(value),
            "can_place_on" => can_place_on = strings_in(value),
            "enchantments" => {
                let inner = value
                    .trim()
                    .trim_start_matches('{')
                    .trim_end_matches('}')
                    .replace("levels:", "")
                    .replace(['{', '}'], "");
                for pair in split_top(&inner) {
                    let Some((name, level)) = pair.rsplit_once(':') else {
                        continue;
                    };
                    let name = name.trim().trim_matches(['"', '\'']);
                    if name == "show_in_tooltip" {
                        continue;
                    }
                    let level: u8 = level
                        .trim()
                        .parse()
                        .map_err(|_| format!("Invalid integer '{}'", level.trim()))?;
                    stack.enchant(enchantment_named(name)?, level);
                }
            }
            other => return Err(format!("Unknown item component '{other}'")),
        }
    }
    for rule in can_break.iter().chain(&can_place_on) {
        let name = rule.strip_prefix("minecraft:").unwrap_or(rule);
        if !name.starts_with('#') && crate::world::blocks::by_name(name).is_none() {
            return Err(format!("Unknown block type '{rule}'"));
        }
    }
    if !can_break.is_empty() || !can_place_on.is_empty() {
        stack.set_block_rules(can_break, can_place_on);
    }
    Ok(())
}

/// `item[components]`: the item and the stack it makes.
pub(super) fn item_with_components(
    text: &str,
) -> Result<(crate::inventory::item::ItemId, ItemStack), String> {
    let (name, components) = match text.split_once('[') {
        Some((name, rest)) => (
            name,
            Some(
                rest.strip_suffix(']')
                    .ok_or("Expected ']' to close the components")?,
            ),
        ),
        None => (text, None),
    };
    let id = item_named(name)?;
    let mut stack = ItemStack::new(id, 1);
    if let Some(components) = components {
        apply(&mut stack, components)?;
    }
    Ok((id, stack))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn components_split_outside_brackets_and_quotes() {
        assert_eq!(
            split_top(r#"damage=5,custom_name='"a, b"',can_break=["x","y"]"#),
            [
                "damage=5",
                r#"custom_name='"a, b"'"#,
                r#"can_break=["x","y"]"#
            ]
        );
    }

    #[test]
    fn lists_names_and_enchantments_parse() {
        assert_eq!(
            strings_in(r#"["stone","minecraft:dirt"]"#),
            ["stone", "minecraft:dirt"]
        );
        assert_eq!(strings_in(r#"{blocks:["stone"]}"#), ["stone"]);
        assert_eq!(strings_in("[stone,dirt]"), ["stone", "dirt"]);
        assert_eq!(plain_text(r#"'"Excalibur"'"#), "Excalibur");
        assert_eq!(plain_text(r#"{"text":"Excalibur"}"#), "Excalibur");
        let (_, sword) = item_with_components(
            r#"diamond_sword[damage=5,custom_name='"Excalibur"',enchantments={levels:{"minecraft:sharpness":3}},can_break=["stone"]]"#,
        )
        .unwrap();
        assert_eq!(sword.damage, 5);
        assert_eq!(sword.custom_name(), Some("Excalibur"));
        assert_eq!(sword.level_of(Enchantment::Sharpness), 3);
        assert_eq!(sword.can_break(), ["stone"]);
        assert!(item_with_components("diamond_sword[bogus=1]").is_err());
        assert!(item_with_components("diamond_sword[can_break=[\"nothing\"]]").is_err());
        assert!(item_with_components("diamond_sword[enchantments={sharpness:1").is_err());
    }
}
