//! NBT values with the text form commands use (SNBT), and paths into them
//! (`Pos[0]`, `Inventory[{Slot:0b}].count`), for `/data` (M44).

use std::collections::BTreeMap;

#[derive(Debug, Clone, PartialEq)]
pub enum Nbt {
    Byte(i8),
    Short(i16),
    Int(i32),
    Long(i64),
    Float(f32),
    Double(f64),
    Str(String),
    List(Vec<Nbt>),
    Compound(BTreeMap<String, Nbt>),
}

impl Nbt {
    pub fn compound(pairs: impl IntoIterator<Item = (&'static str, Nbt)>) -> Nbt {
        Nbt::Compound(pairs.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }

    /// A number as `f64`, if it is one.
    pub fn as_f64(&self) -> Option<f64> {
        Some(match self {
            Nbt::Byte(n) => f64::from(*n),
            Nbt::Short(n) => f64::from(*n),
            Nbt::Int(n) => f64::from(*n),
            Nbt::Long(n) => *n as f64,
            Nbt::Float(n) => f64::from(*n),
            Nbt::Double(n) => *n,
            _ => return None,
        })
    }

    /// A number as a command result: truncated toward zero.
    pub fn as_i32(&self) -> Option<i32> {
        self.as_f64().map(|n| n as i32)
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Nbt::Str(s) => Some(s),
            _ => None,
        }
    }

    pub fn get(&self, key: &str) -> Option<&Nbt> {
        match self {
            Nbt::Compound(map) => map.get(key),
            _ => None,
        }
    }

    /// The text form: `{a:1b,b:"x",c:[1,2]}`.
    pub fn snbt(&self) -> String {
        match self {
            Nbt::Byte(n) => format!("{n}b"),
            Nbt::Short(n) => format!("{n}s"),
            Nbt::Int(n) => n.to_string(),
            Nbt::Long(n) => format!("{n}L"),
            Nbt::Float(n) => format!("{n:?}f"),
            Nbt::Double(n) => format!("{n:?}d"),
            Nbt::Str(s) => quote(s),
            Nbt::List(items) => {
                let inner: Vec<String> = items.iter().map(Nbt::snbt).collect();
                format!("[{}]", inner.join(","))
            }
            Nbt::Compound(map) => {
                let inner: Vec<String> = map
                    .iter()
                    .map(|(k, v)| format!("{}:{}", key_text(k), v.snbt()))
                    .collect();
                format!("{{{}}}", inner.join(","))
            }
        }
    }

    /// Reads the text form, all of it.
    pub fn parse(text: &str) -> Result<Nbt, String> {
        let mut p = Parser {
            chars: text.chars().collect(),
            at: 0,
        };
        let value = p.value()?;
        p.skip();
        if p.at < p.chars.len() {
            return Err(format!("Trailing data at position {}", p.at));
        }
        Ok(value)
    }
}

fn key_text(key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
    {
        key.to_owned()
    } else {
        quote(key)
    }
}

fn quote(s: &str) -> String {
    let double = s.contains('\'') && !s.contains('"');
    let q = if double { '"' } else { '\'' };
    let mut out = String::from(q);
    for c in s.chars() {
        if c == q || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push(q);
    out
}

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn skip(&mut self) {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }

    fn expect(&mut self, c: char) -> Result<(), String> {
        self.skip();
        if self.peek() == Some(c) {
            self.at += 1;
            Ok(())
        } else {
            Err(format!("Expected '{c}' at position {}", self.at))
        }
    }

    fn value(&mut self) -> Result<Nbt, String> {
        self.skip();
        match self.peek() {
            Some('{') => self.compound(),
            Some('[') => self.list(),
            Some('"' | '\'') => Ok(Nbt::Str(self.quoted()?)),
            Some(_) => {
                let word = self.bare();
                if word.is_empty() {
                    return Err(format!("Expected a value at position {}", self.at));
                }
                Ok(number(&word).unwrap_or(Nbt::Str(word)))
            }
            None => Err("Expected a value".to_owned()),
        }
    }

    fn bare(&mut self) -> String {
        let start = self.at;
        while self
            .peek()
            .is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '+'))
        {
            self.at += 1;
        }
        self.chars[start..self.at].iter().collect()
    }

    fn quoted(&mut self) -> Result<String, String> {
        let q = self.peek().ok_or("Expected a string")?;
        self.at += 1;
        let mut out = String::new();
        loop {
            match self.peek() {
                None => return Err("Unterminated string".to_owned()),
                Some('\\') => {
                    self.at += 1;
                    if let Some(c) = self.peek() {
                        out.push(c);
                        self.at += 1;
                    }
                }
                Some(c) if c == q => {
                    self.at += 1;
                    return Ok(out);
                }
                Some(c) => {
                    out.push(c);
                    self.at += 1;
                }
            }
        }
    }

    fn compound(&mut self) -> Result<Nbt, String> {
        self.expect('{')?;
        let mut map = BTreeMap::new();
        self.skip();
        if self.peek() == Some('}') {
            self.at += 1;
            return Ok(Nbt::Compound(map));
        }
        loop {
            self.skip();
            let key = match self.peek() {
                Some('"' | '\'') => self.quoted()?,
                _ => self.bare(),
            };
            if key.is_empty() {
                return Err(format!("Expected a key at position {}", self.at));
            }
            self.expect(':')?;
            let value = self.value()?;
            map.insert(key, value);
            self.skip();
            match self.peek() {
                Some(',') => self.at += 1,
                Some('}') => {
                    self.at += 1;
                    return Ok(Nbt::Compound(map));
                }
                _ => return Err(format!("Expected ',' or '}}' at position {}", self.at)),
            }
        }
    }

    fn list(&mut self) -> Result<Nbt, String> {
        self.expect('[')?;
        self.skip();
        // `[B;...]`, `[I;...]` and `[L;...]` arrays are kept as lists.
        if matches!(self.peek(), Some('B' | 'I' | 'L')) && self.chars.get(self.at + 1) == Some(&';')
        {
            self.at += 2;
        }
        let mut items = Vec::new();
        self.skip();
        if self.peek() == Some(']') {
            self.at += 1;
            return Ok(Nbt::List(items));
        }
        loop {
            items.push(self.value()?);
            self.skip();
            match self.peek() {
                Some(',') => self.at += 1,
                Some(']') => {
                    self.at += 1;
                    return Ok(Nbt::List(items));
                }
                _ => return Err(format!("Expected ',' or ']' at position {}", self.at)),
            }
        }
    }
}

/// A number word with its suffix, `true` and `false`.
fn number(word: &str) -> Option<Nbt> {
    match word {
        "true" => return Some(Nbt::Byte(1)),
        "false" => return Some(Nbt::Byte(0)),
        _ => {}
    }
    let (digits, suffix) = match word.chars().last()? {
        c @ ('b' | 'B' | 's' | 'S' | 'l' | 'L' | 'f' | 'F' | 'd' | 'D') => {
            (&word[..word.len() - 1], Some(c.to_ascii_lowercase()))
        }
        _ => (word, None),
    };
    if digits.is_empty()
        || !digits.chars().next()?.is_ascii_digit()
            && !matches!(digits.chars().next()?, '-' | '+' | '.')
    {
        return None;
    }
    Some(match suffix {
        Some('b') => Nbt::Byte(digits.parse().ok()?),
        Some('s') => Nbt::Short(digits.parse().ok()?),
        Some('l') => Nbt::Long(digits.parse().ok()?),
        Some('f') => Nbt::Float(digits.parse().ok()?),
        Some('d') => Nbt::Double(digits.parse().ok()?),
        _ => {
            if let Ok(n) = digits.parse::<i32>() {
                Nbt::Int(n)
            } else {
                Nbt::Double(digits.parse().ok()?)
            }
        }
    })
}

/// One step of a path.
#[derive(Debug, Clone, PartialEq)]
pub enum Step {
    Key(String),
    /// `[n]`, negative counting from the end.
    Index(i32),
    /// `[{...}]`: the list elements that contain this.
    Match(Nbt),
    /// `[]`: every element.
    All,
    /// `{...}` at the start: the root, if it contains this.
    Root(Nbt),
}

/// Parses `a.b[0].c`, `list[{id:"x"}]`, `list[]` and `{key:value}`.
pub fn parse_path(text: &str) -> Result<Vec<Step>, String> {
    let chars: Vec<char> = text.chars().collect();
    let mut at = 0;
    let mut steps = Vec::new();
    let bad = |at: usize| format!("Invalid NBT path at position {at}");
    while at < chars.len() {
        match chars[at] {
            '.' => {
                at += 1;
                if at >= chars.len() {
                    return Err(bad(at));
                }
            }
            '[' => {
                at += 1;
                match chars.get(at) {
                    Some(']') => {
                        steps.push(Step::All);
                        at += 1;
                    }
                    Some('{') => {
                        let end = matching(&chars, at, '{', '}').ok_or_else(|| bad(at))?;
                        let inner: String = chars[at..=end].iter().collect();
                        steps.push(Step::Match(Nbt::parse(&inner)?));
                        at = end + 1;
                        if chars.get(at) != Some(&']') {
                            return Err(bad(at));
                        }
                        at += 1;
                    }
                    _ => {
                        let start = at;
                        while chars.get(at).is_some_and(|c| *c != ']') {
                            at += 1;
                        }
                        let n: String = chars[start..at].iter().collect();
                        steps.push(Step::Index(n.trim().parse().map_err(|_| bad(start))?));
                        if chars.get(at) != Some(&']') {
                            return Err(bad(at));
                        }
                        at += 1;
                    }
                }
            }
            '{' if steps.is_empty() => {
                let end = matching(&chars, at, '{', '}').ok_or_else(|| bad(at))?;
                let inner: String = chars[at..=end].iter().collect();
                steps.push(Step::Root(Nbt::parse(&inner)?));
                at = end + 1;
            }
            '"' | '\'' => {
                let q = chars[at];
                at += 1;
                let start = at;
                while chars.get(at).is_some_and(|c| *c != q) {
                    at += 1;
                }
                steps.push(Step::Key(chars[start..at].iter().collect()));
                at += 1;
            }
            _ => {
                let start = at;
                while chars.get(at).is_some_and(|c| !matches!(c, '.' | '[' | '{')) {
                    at += 1;
                }
                steps.push(Step::Key(chars[start..at].iter().collect()));
                if chars.get(at) == Some(&'{') {
                    // `key{...}` matches a compound child.
                    let end = matching(&chars, at, '{', '}').ok_or_else(|| bad(at))?;
                    let inner: String = chars[at..=end].iter().collect();
                    steps.push(Step::Root(Nbt::parse(&inner)?));
                    at = end + 1;
                }
            }
        }
    }
    if steps.is_empty() {
        return Err(bad(0));
    }
    Ok(steps)
}

fn matching(chars: &[char], open_at: usize, open: char, close: char) -> Option<usize> {
    let (mut depth, mut quote) = (0, None::<char>);
    for (i, &c) in chars.iter().enumerate().skip(open_at) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if c == '"' || c == '\'' => quote = Some(c),
            None if c == open => depth += 1,
            None if c == close => {
                depth -= 1;
                if depth == 0 {
                    return Some(i);
                }
            }
            None => {}
        }
    }
    None
}

/// Whether `value` contains everything `pattern` does (compounds by key,
/// lists by having a matching element for each, others equal).
pub fn contains(value: &Nbt, pattern: &Nbt) -> bool {
    match (value, pattern) {
        (Nbt::Compound(v), Nbt::Compound(p)) => p
            .iter()
            .all(|(k, pv)| v.get(k).is_some_and(|vv| contains(vv, pv))),
        (Nbt::List(v), Nbt::List(p)) => p.iter().all(|pe| v.iter().any(|ve| contains(ve, pe))),
        (a, b) => match (a.as_f64(), b.as_f64()) {
            (Some(x), Some(y)) => (x - y).abs() < 1e-9,
            _ => a == b,
        },
    }
}

/// Every value `path` reaches in `root`.
pub fn get_all<'a>(root: &'a Nbt, path: &[Step]) -> Vec<&'a Nbt> {
    let mut current = vec![root];
    for step in path {
        let mut next = Vec::new();
        for node in current {
            match (step, node) {
                (Step::Key(k), Nbt::Compound(map)) => next.extend(map.get(k)),
                (Step::Root(p), n) if contains(n, p) => next.push(n),
                (Step::Index(i), Nbt::List(items)) => {
                    let at = if *i < 0 { items.len() as i32 + i } else { *i };
                    next.extend(usize::try_from(at).ok().and_then(|a| items.get(a)));
                }
                (Step::Match(p), Nbt::List(items)) => {
                    next.extend(items.iter().filter(|e| contains(e, p)));
                }
                (Step::All, Nbt::List(items)) => next.extend(items),
                _ => {}
            }
        }
        current = next;
    }
    current
}

/// Applies `f` to every value `path` reaches, mutably; returns how many.
pub fn for_each_mut(root: &mut Nbt, path: &[Step], f: &mut impl FnMut(&mut Nbt)) -> usize {
    let Some((step, rest)) = path.split_first() else {
        f(root);
        return 1;
    };
    match (step, root) {
        (Step::Key(k), Nbt::Compound(map)) => map
            .get_mut(k)
            .map_or(0, |child| for_each_mut(child, rest, f)),
        (Step::Root(p), n) if contains(n, p) => for_each_mut(n, rest, f),
        (Step::Index(i), Nbt::List(items)) => {
            let len = items.len() as i32;
            let at = if *i < 0 { len + i } else { *i };
            usize::try_from(at)
                .ok()
                .and_then(|a| items.get_mut(a))
                .map_or(0, |child| for_each_mut(child, rest, f))
        }
        (Step::Match(p), Nbt::List(items)) => items
            .iter_mut()
            .filter(|e| contains(e, p))
            .map(|e| for_each_mut(e, rest, f))
            .sum(),
        (Step::All, Nbt::List(items)) => items.iter_mut().map(|e| for_each_mut(e, rest, f)).sum(),
        _ => 0,
    }
}

/// Sets what `path` names to `value`, creating compounds along the way for
/// keys. Returns whether anything changed.
pub fn set(root: &mut Nbt, path: &[Step], value: &Nbt) -> bool {
    let (last, parents) = path.split_last().expect("a path has a step");
    // Make missing compound keys.
    if let Step::Key(_) = last {
        let mut node: &mut Nbt = root;
        for step in parents {
            match step {
                Step::Key(k) => {
                    let Nbt::Compound(map) = node else {
                        return false;
                    };
                    node = map
                        .entry(k.clone())
                        .or_insert_with(|| Nbt::Compound(BTreeMap::new()));
                }
                _ => break,
            }
        }
    }
    let mut changed = false;
    if parents.is_empty() {
        return match (last, &mut *root) {
            (Step::Key(k), Nbt::Compound(map)) => {
                let before = map.insert(k.clone(), value.clone());
                before.as_ref() != Some(value)
            }
            _ => false,
        };
    }
    for_each_mut(root, parents, &mut |parent| match (last, parent) {
        (Step::Key(k), Nbt::Compound(map)) => {
            let before = map.insert(k.clone(), value.clone());
            changed |= before.as_ref() != Some(value);
        }
        (Step::Index(i), Nbt::List(items)) => {
            let len = items.len() as i32;
            let at = if *i < 0 { len + i } else { *i };
            if let Some(slot) = usize::try_from(at).ok().and_then(|a| items.get_mut(a)) {
                changed |= slot != value;
                *slot = value.clone();
            }
        }
        (Step::Match(p), Nbt::List(items)) => {
            for e in items.iter_mut().filter(|e| contains(e, p)) {
                changed |= e != value;
                *e = value.clone();
            }
        }
        (Step::All, Nbt::List(items)) => {
            for e in items.iter_mut() {
                changed |= e != value;
                *e = value.clone();
            }
        }
        _ => {}
    });
    changed
}

/// Removes what `path` names; returns how many were removed.
pub fn remove(root: &mut Nbt, path: &[Step]) -> usize {
    let (last, parents) = path.split_last().expect("a path has a step");
    let mut removed = 0;
    let mut take = |parent: &mut Nbt| match (last, parent) {
        (Step::Key(k), Nbt::Compound(map)) => removed += usize::from(map.remove(k).is_some()),
        (Step::Index(i), Nbt::List(items)) => {
            let len = items.len() as i32;
            let at = if *i < 0 { len + i } else { *i };
            if let Ok(a) = usize::try_from(at)
                && a < items.len()
            {
                items.remove(a);
                removed += 1;
            }
        }
        (Step::Match(p), Nbt::List(items)) => {
            let before = items.len();
            items.retain(|e| !contains(e, p));
            removed += before - items.len();
        }
        (Step::All, Nbt::List(items)) => {
            removed += items.len();
            items.clear();
        }
        _ => {}
    };
    if parents.is_empty() {
        take(root);
    } else {
        for_each_mut(root, parents, &mut take);
    }
    removed
}

/// Merges `from` into `into`: compounds key by key, anything else replaced.
pub fn merge(into: &mut Nbt, from: &Nbt) -> bool {
    match (into, from) {
        (Nbt::Compound(a), Nbt::Compound(b)) => {
            let mut changed = false;
            for (k, v) in b {
                match a.get_mut(k) {
                    Some(existing) => changed |= merge(existing, v),
                    None => {
                        a.insert(k.clone(), v.clone());
                        changed = true;
                    }
                }
            }
            changed
        }
        (a, b) if a != b => {
            *a = b.clone();
            true
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nbt(text: &str) -> Nbt {
        Nbt::parse(text).unwrap()
    }

    #[test]
    fn snbt_reads_every_kind_and_prints_it_back() {
        let v = nbt(
            r#"{a:1b,b:2s,c:3,d:4L,e:1.5f,f:2.5d,g:"hi",h:'it''s',i:[1,2],j:{k:true},"l m":0}"#
                .replace("''", "\\'")
                .as_str(),
        );
        assert_eq!(v.get("a"), Some(&Nbt::Byte(1)));
        assert_eq!(v.get("b"), Some(&Nbt::Short(2)));
        assert_eq!(v.get("c"), Some(&Nbt::Int(3)));
        assert_eq!(v.get("d"), Some(&Nbt::Long(4)));
        assert_eq!(v.get("e"), Some(&Nbt::Float(1.5)));
        assert_eq!(v.get("f"), Some(&Nbt::Double(2.5)));
        assert_eq!(v.get("g"), Some(&Nbt::Str("hi".into())));
        assert_eq!(v.get("h"), Some(&Nbt::Str("it's".into())));
        assert_eq!(v.get("j").unwrap().get("k"), Some(&Nbt::Byte(1)));
        let again = nbt(&v.snbt());
        assert_eq!(again, v, "printing and reading agree: {}", v.snbt());
        assert_eq!(
            nbt("[B;1b,2b]"),
            Nbt::List(vec![Nbt::Byte(1), Nbt::Byte(2)])
        );
        assert_eq!(nbt("1.5"), Nbt::Double(1.5));
        assert_eq!(nbt("-3"), Nbt::Int(-3));
        assert_eq!(nbt("hello"), Nbt::Str("hello".into()));
        for bad in ["{a:}", "{a:1", "[1,2", "{a 1}", "\"open", "{a:1} x"] {
            assert!(Nbt::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn paths_get_set_remove_and_match() {
        let mut v = nbt(
            r#"{Pos:[1.0d,2.0d,3.0d],Items:[{Slot:0b,id:"a",n:1},{Slot:1b,id:"b",n:2}],tag:{x:5}}"#,
        );
        let p = |t: &str| parse_path(t).unwrap();
        assert_eq!(get_all(&v, &p("Pos[1]")), [&Nbt::Double(2.0)]);
        assert_eq!(get_all(&v, &p("Pos[-1]")), [&Nbt::Double(3.0)]);
        assert_eq!(get_all(&v, &p("Items[{id:\"b\"}].n")), [&Nbt::Int(2)]);
        assert_eq!(get_all(&v, &p("Items[].Slot")).len(), 2);
        assert_eq!(get_all(&v, &p("tag.x")), [&Nbt::Int(5)]);
        assert!(get_all(&v, &p("tag.y")).is_empty());
        assert_eq!(get_all(&v, &p("tag{x:5}.x")), [&Nbt::Int(5)]);
        assert!(get_all(&v, &p("tag{x:6}.x")).is_empty());
        assert!(set(&mut v, &p("tag.x"), &Nbt::Int(9)));
        assert!(!set(&mut v, &p("tag.x"), &Nbt::Int(9)), "unchanged");
        assert!(
            set(&mut v, &p("tag.deep.er"), &Nbt::Byte(1)),
            "makes the compounds"
        );
        assert!(set(&mut v, &p("Items[0].n"), &Nbt::Int(7)));
        assert!(set(&mut v, &p("Pos[0]"), &Nbt::Double(0.0)));
        assert_eq!(get_all(&v, &p("Items[0].n")), [&Nbt::Int(7)]);
        assert_eq!(remove(&mut v, &p("Items[{id:\"a\"}]")), 1);
        assert_eq!(get_all(&v, &p("Items[].id")).len(), 1);
        assert_eq!(remove(&mut v, &p("tag")), 1);
        assert_eq!(remove(&mut v, &p("tag")), 0);
        assert!(parse_path("").is_err() && parse_path("a[").is_err() && parse_path("a.").is_err());
    }

    #[test]
    fn merge_and_containment() {
        let mut a = nbt("{x:{y:1,z:2},list:[1]}");
        assert!(merge(&mut a, &nbt("{x:{y:5},w:1}")));
        assert_eq!(a, nbt("{x:{y:5,z:2},w:1,list:[1]}"));
        assert!(!merge(&mut a, &nbt("{x:{y:5}}")));
        assert!(contains(&a, &nbt("{x:{z:2}}")));
        assert!(!contains(&a, &nbt("{x:{z:3}}")));
        assert!(
            contains(&nbt("{n:1b}"), &nbt("{n:1}")),
            "numbers compare by value"
        );
        assert!(contains(&nbt("{l:[1,2,3]}"), &nbt("{l:[3,1]}")));
    }
}
