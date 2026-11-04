//! The expression language of OptiFine's custom entity models (CEM),
//! which Fresh Animations' animations are written in: `target = expression`
//! lines with numbers, the entity's variables (`limb_swing`, `head_yaw`),
//! variables the model keeps (`var.x`, `varb.flag`), the model's parts
//! (`head.rx`), operators and functions (`sin`, `if`, `lerp`...). Every
//! expression is parsed once, when the model loads, into a small stack
//! bytecode with the constants folded, and evaluated with no allocation
//! each frame.

use std::fmt;

/// Slots in the evaluation stack: more than any expression needs.
const STACK: usize = 48;

/// A built-in function.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Func {
    Sin,
    Cos,
    Tan,
    Asin,
    Acos,
    Atan,
    Atan2,
    Torad,
    Todeg,
    Min,
    Max,
    Clamp,
    Abs,
    Floor,
    Ceil,
    Exp,
    Frac,
    Log,
    Pow,
    Random,
    Round,
    Signum,
    Sqrt,
    Fmod,
    If,
    Lerp,
    Between,
    Equals,
    In,
    Print,
}

impl Func {
    /// The function a name means, and how many arguments it takes at
    /// least and at most.
    fn named(name: &str) -> Option<(Self, usize, usize)> {
        Some(match name {
            "sin" => (Func::Sin, 1, 1),
            "cos" => (Func::Cos, 1, 1),
            "tan" => (Func::Tan, 1, 1),
            "asin" => (Func::Asin, 1, 1),
            "acos" => (Func::Acos, 1, 1),
            "atan" => (Func::Atan, 1, 1),
            "atan2" => (Func::Atan2, 2, 2),
            "torad" => (Func::Torad, 1, 1),
            "todeg" => (Func::Todeg, 1, 1),
            "min" => (Func::Min, 2, 32),
            "max" => (Func::Max, 2, 32),
            "clamp" => (Func::Clamp, 3, 3),
            "abs" => (Func::Abs, 1, 1),
            "floor" => (Func::Floor, 1, 1),
            "ceil" => (Func::Ceil, 1, 1),
            "exp" => (Func::Exp, 1, 1),
            "frac" => (Func::Frac, 1, 1),
            "log" => (Func::Log, 1, 1),
            "pow" => (Func::Pow, 2, 2),
            "random" => (Func::Random, 0, 1),
            "round" => (Func::Round, 1, 1),
            "signum" => (Func::Signum, 1, 1),
            "sqrt" => (Func::Sqrt, 1, 1),
            "fmod" => (Func::Fmod, 2, 2),
            "if" | "ifb" => (Func::If, 3, 33),
            "lerp" => (Func::Lerp, 3, 3),
            "between" => (Func::Between, 3, 3),
            "equals" => (Func::Equals, 3, 3),
            "in" => (Func::In, 2, 32),
            "print" | "printb" => (Func::Print, 1, 32),
            _ => return None,
        })
    }

    /// Whether the same arguments always give the same result (so a call
    /// with constant arguments can be folded away).
    fn pure(self) -> bool {
        !matches!(self, Func::Random | Func::Print)
    }
}

/// One step of the bytecode.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Op {
    Const(f32),
    /// Pushes a variable's value.
    Load(Slot),
    Neg,
    Not,
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Lt,
    Le,
    Gt,
    Ge,
    Eq,
    Ne,
    And,
    Or,
    /// Calls a function with this many arguments from the stack.
    Call(Func, u8),
}

/// Where a variable lives. Entity variables and parts' fields are filled
/// in by the model; `Var` and `VarB` persist between frames per mob.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Entity(u8),
    Var(u16),
    Part(u16, PartField),
    Render(RenderField),
}

/// What of a part an animation reads or writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PartField {
    Tx,
    Ty,
    Tz,
    Rx,
    Ry,
    Rz,
    Sx,
    Sy,
    Sz,
    Visible,
}

impl PartField {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "tx" => PartField::Tx,
            "ty" => PartField::Ty,
            "tz" => PartField::Tz,
            "rx" => PartField::Rx,
            "ry" => PartField::Ry,
            "rz" => PartField::Rz,
            "sx" => PartField::Sx,
            "sy" => PartField::Sy,
            "sz" => PartField::Sz,
            "visible" => PartField::Visible,
            _ => return None,
        })
    }
}

/// What of the whole model an animation reads or writes (`render.*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderField {
    ShadowSize,
    ShadowOpacity,
    ModelScale,
}

impl RenderField {
    pub fn named(name: &str) -> Option<Self> {
        Some(match name {
            "shadow_size" => RenderField::ShadowSize,
            "shadow_opacity" => RenderField::ShadowOpacity,
            "model_scale" | "scale" => RenderField::ModelScale,
            _ => return None,
        })
    }
}

/// Turns a name in an expression into a slot, or says it isn't known.
pub trait Resolver {
    fn resolve(&mut self, name: &str) -> Option<Slot>;
}

/// An expression that doesn't parse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseError(pub String);

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for ParseError {}

fn err<T>(message: impl Into<String>) -> Result<T, ParseError> {
    Err(ParseError(message.into()))
}

#[derive(Debug, Clone, PartialEq)]
enum Token {
    Number(f32),
    Name(String),
    Op(&'static str),
    Open,
    Close,
    Comma,
}

fn tokenize(text: &str) -> Result<Vec<Token>, ParseError> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
        } else if c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(char::is_ascii_digit))
        {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
            // An exponent: 1e-3.
            if i < chars.len()
                && (chars[i] == 'e' || chars[i] == 'E')
                && chars
                    .get(i + 1)
                    .is_some_and(|n| n.is_ascii_digit() || *n == '-' || *n == '+')
            {
                i += 2;
                while i < chars.len() && chars[i].is_ascii_digit() {
                    i += 1;
                }
            }
            let word: String = chars[start..i].iter().collect();
            // A trailing `f` or `F` (3.5f) is allowed.
            if i < chars.len() && (chars[i] == 'f' || chars[i] == 'F') {
                i += 1;
            }
            match word.parse() {
                Ok(n) => tokens.push(Token::Number(n)),
                Err(_) => return err(format!("bad number {word:?}")),
            }
        } else if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len()
                && (chars[i].is_alphanumeric() || chars[i] == '_' || chars[i] == '.')
            {
                i += 1;
            }
            tokens.push(Token::Name(chars[start..i].iter().collect()));
        } else {
            let two: String = chars[i..chars.len().min(i + 2)].iter().collect();
            let op = match two.as_str() {
                "<=" => Some("<="),
                ">=" => Some(">="),
                "==" => Some("=="),
                "!=" => Some("!="),
                "&&" => Some("&&"),
                "||" => Some("||"),
                _ => None,
            };
            if let Some(op) = op {
                tokens.push(Token::Op(op));
                i += 2;
                continue;
            }
            tokens.push(match c {
                '+' => Token::Op("+"),
                '-' => Token::Op("-"),
                '*' => Token::Op("*"),
                '/' => Token::Op("/"),
                '%' => Token::Op("%"),
                '<' => Token::Op("<"),
                '>' => Token::Op(">"),
                '!' => Token::Op("!"),
                '(' => Token::Open,
                ')' => Token::Close,
                ',' => Token::Comma,
                other => return err(format!("unexpected {other:?}")),
            });
            i += 1;
        }
    }
    Ok(tokens)
}

/// A parsed expression.
#[derive(Debug, Clone, PartialEq)]
pub struct Program {
    ops: Vec<Op>,
}

impl Program {
    /// The bytecode (for tests and the model's report).
    pub fn ops(&self) -> &[Op] {
        &self.ops
    }

    /// The value, if the whole expression folded to a constant.
    pub fn constant(&self) -> Option<f32> {
        match self.ops.as_slice() {
            [Op::Const(v)] => Some(*v),
            _ => None,
        }
    }

    /// Parses `text`, looking names up with `resolver`. A name the
    /// resolver doesn't know reads as 0 and is passed to `unknown`.
    pub fn parse(
        text: &str,
        resolver: &mut impl Resolver,
        unknown: &mut impl FnMut(&str),
    ) -> Result<Self, ParseError> {
        let tokens = tokenize(text)?;
        let mut parser = Parser {
            tokens: &tokens,
            at: 0,
            ops: Vec::new(),
            resolver,
            unknown,
            depth: 0,
        };
        parser.expression(0)?;
        if parser.at != tokens.len() {
            return err(format!(
                "unexpected {:?} after the expression",
                tokens[parser.at]
            ));
        }
        let ops = parser.ops;
        if stack_needed(&ops) > STACK {
            return err("expression too deep");
        }
        Ok(Self { ops })
    }

    /// Runs it. `read` gives a slot's value; `random` the next random
    /// number in 0–1.
    pub fn eval(
        &self,
        read: &mut impl FnMut(Slot) -> f32,
        random: &mut impl FnMut() -> f32,
    ) -> f32 {
        let mut stack = [0.0f32; STACK];
        let mut top = 0;
        for op in &self.ops {
            match *op {
                Op::Const(v) => {
                    stack[top] = v;
                    top += 1;
                }
                Op::Load(slot) => {
                    stack[top] = read(slot);
                    top += 1;
                }
                Op::Neg => stack[top - 1] = -stack[top - 1],
                Op::Not => stack[top - 1] = f32::from(stack[top - 1] == 0.0),
                Op::Call(func, argc) => {
                    let argc = usize::from(argc);
                    let args = &stack[top - argc..top];
                    let value = call(func, args, random);
                    top -= argc;
                    stack[top] = value;
                    top += 1;
                }
                binary => {
                    let (a, b) = (stack[top - 2], stack[top - 1]);
                    top -= 1;
                    stack[top - 1] = binary_op(binary, a, b);
                }
            }
        }
        if top == 0 { 0.0 } else { stack[top - 1] }
    }
}

fn binary_op(op: Op, a: f32, b: f32) -> f32 {
    match op {
        Op::Add => a + b,
        Op::Sub => a - b,
        Op::Mul => a * b,
        // Dividing by zero gives 0 rather than infinity: an animation
        // that does it should still pose the model.
        Op::Div => {
            if b == 0.0 {
                0.0
            } else {
                a / b
            }
        }
        Op::Rem => {
            if b == 0.0 {
                0.0
            } else {
                a - b * (a / b).floor()
            }
        }
        Op::Lt => f32::from(a < b),
        Op::Le => f32::from(a <= b),
        Op::Gt => f32::from(a > b),
        Op::Ge => f32::from(a >= b),
        Op::Eq => f32::from(a == b),
        Op::Ne => f32::from(a != b),
        Op::And => f32::from(a != 0.0 && b != 0.0),
        Op::Or => f32::from(a != 0.0 || b != 0.0),
        _ => 0.0,
    }
}

/// A number in 0..1 that depends only on `seed`'s bits (a murmur-style
/// finalizer), never on the frame or the mob's random sequence.
pub fn seeded_random(seed: f32) -> f32 {
    let mut h = seed.to_bits();
    h ^= h >> 16;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^= h >> 16;
    (h >> 8) as f32 / (1u32 << 24) as f32
}

fn call(func: Func, a: &[f32], random: &mut impl FnMut() -> f32) -> f32 {
    match func {
        Func::Sin => a[0].sin(),
        Func::Cos => a[0].cos(),
        Func::Tan => a[0].tan(),
        Func::Asin => a[0].clamp(-1.0, 1.0).asin(),
        Func::Acos => a[0].clamp(-1.0, 1.0).acos(),
        Func::Atan => a[0].atan(),
        Func::Atan2 => a[0].atan2(a[1]),
        Func::Torad => a[0].to_radians(),
        Func::Todeg => a[0].to_degrees(),
        Func::Min => a.iter().copied().fold(f32::INFINITY, f32::min),
        Func::Max => a.iter().copied().fold(f32::NEG_INFINITY, f32::max),
        Func::Clamp => a[0].max(a[1]).min(a[2].max(a[1])),
        Func::Abs => a[0].abs(),
        Func::Floor => a[0].floor(),
        Func::Ceil => a[0].ceil(),
        Func::Exp => a[0].exp(),
        Func::Frac => a[0] - a[0].floor(),
        Func::Log => {
            if a[0] > 0.0 {
                a[0].ln()
            } else {
                0.0
            }
        }
        Func::Pow => {
            let p = a[0].powf(a[1]);
            if p.is_finite() { p } else { 0.0 }
        }
        // random(seed) is the same number for the same seed, on every
        // frame and every client (OptiFine's cem_animation.txt); only
        // random() with no argument draws a fresh one.
        Func::Random => match a.first() {
            Some(seed) => seeded_random(*seed),
            None => random(),
        },
        Func::Round => a[0].round(),
        Func::Signum => {
            if a[0] > 0.0 {
                1.0
            } else if a[0] < 0.0 {
                -1.0
            } else {
                0.0
            }
        }
        Func::Sqrt => a[0].max(0.0).sqrt(),
        Func::Fmod => {
            if a[1] == 0.0 {
                0.0
            } else {
                a[0] - a[1] * (a[0] / a[1]).floor()
            }
        }
        // if(cond, then, else) and the chained if(c1, v1, c2, v2, else).
        Func::If => {
            let mut i = 0;
            while i + 1 < a.len() {
                if a[i] != 0.0 {
                    return a[i + 1];
                }
                i += 2;
            }
            if i < a.len() { a[i] } else { 0.0 }
        }
        Func::Lerp => a[1] + (a[2] - a[1]) * a[0],
        Func::Between => f32::from(a[0] >= a[1] && a[0] <= a[2]),
        Func::Equals => f32::from((a[0] - a[1]).abs() <= a[2].abs()),
        Func::In => f32::from(a[1..].iter().any(|v| *v == a[0])),
        Func::Print => a[a.len() - 1],
    }
}

/// How deep the stack gets running `ops`.
fn stack_needed(ops: &[Op]) -> usize {
    let (mut depth, mut deepest) = (0usize, 0usize);
    for op in ops {
        match op {
            Op::Const(_) | Op::Load(_) => depth += 1,
            Op::Neg | Op::Not => {}
            Op::Call(_, argc) => depth = depth + 1 - usize::from(*argc),
            _ => depth -= 1,
        }
        deepest = deepest.max(depth);
    }
    deepest
}

struct Parser<'a, R, U> {
    tokens: &'a [Token],
    at: usize,
    ops: Vec<Op>,
    resolver: &'a mut R,
    unknown: &'a mut U,
    depth: usize,
}

impl<R: Resolver, U: FnMut(&str)> Parser<'_, R, U> {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.at)
    }

    /// The binding power of a binary operator token.
    fn binding(op: &str) -> Option<(u8, Op)> {
        Some(match op {
            "||" => (1, Op::Or),
            "&&" => (2, Op::And),
            "==" => (3, Op::Eq),
            "!=" => (3, Op::Ne),
            "<" => (4, Op::Lt),
            "<=" => (4, Op::Le),
            ">" => (4, Op::Gt),
            ">=" => (4, Op::Ge),
            "+" => (5, Op::Add),
            "-" => (5, Op::Sub),
            "*" => (6, Op::Mul),
            "/" => (6, Op::Div),
            "%" => (6, Op::Rem),
            _ => return None,
        })
    }

    /// Emits `op`, folding it into a constant if its operands are.
    fn emit(&mut self, op: Op) {
        let len = self.ops.len();
        let folded = match op {
            Op::Neg | Op::Not => match self.ops.last() {
                Some(Op::Const(v)) => Some(if op == Op::Neg {
                    -v
                } else {
                    f32::from(*v == 0.0)
                }),
                _ => None,
            },
            Op::Call(func, argc) if func.pure() => {
                let n = usize::from(argc);
                let constants: Option<Vec<f32>> = (len >= n)
                    .then(|| {
                        self.ops[len - n..]
                            .iter()
                            .map(|o| match o {
                                Op::Const(v) => Some(*v),
                                _ => None,
                            })
                            .collect()
                    })
                    .flatten();
                constants.map(|args| call(func, &args, &mut || 0.0))
            }
            Op::Const(_) | Op::Load(_) | Op::Call(..) => None,
            binary => match (len >= 2).then(|| (&self.ops[len - 2], &self.ops[len - 1])) {
                Some((Op::Const(a), Op::Const(b))) => Some(binary_op(binary, *a, *b)),
                _ => None,
            },
        };
        match folded {
            Some(value) => {
                let n = match op {
                    Op::Neg | Op::Not => 1,
                    Op::Call(_, argc) => usize::from(argc),
                    _ => 2,
                };
                self.ops.truncate(len - n);
                self.ops.push(Op::Const(value));
            }
            None => self.ops.push(op),
        }
    }

    fn expression(&mut self, min: u8) -> Result<(), ParseError> {
        self.depth += 1;
        if self.depth > 64 {
            return err("expression nested too deep");
        }
        self.unary()?;
        while let Some(Token::Op(op)) = self.peek() {
            let Some((power, bytecode)) = Self::binding(op) else {
                break;
            };
            if power < min {
                break;
            }
            self.at += 1;
            self.expression(power + 1)?;
            self.emit(bytecode);
        }
        self.depth -= 1;
        Ok(())
    }

    fn unary(&mut self) -> Result<(), ParseError> {
        match self.peek() {
            Some(Token::Op("-")) => {
                self.at += 1;
                self.unary()?;
                self.emit(Op::Neg);
                Ok(())
            }
            Some(Token::Op("+")) => {
                self.at += 1;
                self.unary()
            }
            Some(Token::Op("!")) => {
                self.at += 1;
                self.unary()?;
                self.emit(Op::Not);
                Ok(())
            }
            _ => self.primary(),
        }
    }

    fn primary(&mut self) -> Result<(), ParseError> {
        let Some(token) = self.peek().cloned() else {
            return err("the expression ends too soon");
        };
        self.at += 1;
        match token {
            Token::Number(n) => self.ops.push(Op::Const(n)),
            Token::Open => {
                self.expression(0)?;
                if self.peek() != Some(&Token::Close) {
                    return err("a ( is never closed");
                }
                self.at += 1;
            }
            Token::Name(name) => {
                if self.peek() == Some(&Token::Open) {
                    self.call(&name)?;
                } else {
                    match name.as_str() {
                        "true" => self.ops.push(Op::Const(1.0)),
                        "false" => self.ops.push(Op::Const(0.0)),
                        "pi" => self.ops.push(Op::Const(std::f32::consts::PI)),
                        _ => match self.resolver.resolve(&name) {
                            Some(slot) => self.ops.push(Op::Load(slot)),
                            None => {
                                (self.unknown)(&name);
                                self.ops.push(Op::Const(0.0));
                            }
                        },
                    }
                }
            }
            other => return err(format!("unexpected {other:?}")),
        }
        Ok(())
    }

    fn call(&mut self, name: &str) -> Result<(), ParseError> {
        let Some((func, min, max)) = Func::named(name) else {
            return err(format!("no function {name:?}"));
        };
        self.at += 1; // the (
        let mut argc = 0;
        if self.peek() == Some(&Token::Close) {
            self.at += 1;
        } else {
            loop {
                self.expression(0)?;
                argc += 1;
                match self.peek() {
                    Some(Token::Comma) => self.at += 1,
                    Some(Token::Close) => {
                        self.at += 1;
                        break;
                    }
                    _ => return err(format!("{name}( is never closed")),
                }
            }
        }
        if argc < min || argc > max {
            return err(format!("{name} takes {min} to {max} arguments, not {argc}"));
        }
        self.emit(Op::Call(func, argc as u8));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A resolver with `x` (7), `y` (2), `t` (0.5) and `flag` (1).
    struct Test;
    impl Resolver for Test {
        fn resolve(&mut self, name: &str) -> Option<Slot> {
            Some(match name {
                "x" => Slot::Entity(0),
                "y" => Slot::Entity(1),
                "t" => Slot::Entity(2),
                "flag" => Slot::Entity(3),
                "var.n" => Slot::Var(0),
                _ => return None,
            })
        }
    }

    fn run(text: &str) -> f32 {
        let program = Program::parse(text, &mut Test, &mut |_| {}).unwrap();
        let mut vars = [0.0f32; 4];
        program.eval(
            &mut |slot| match slot {
                Slot::Entity(i) => [7.0, 2.0, 0.5, 1.0][usize::from(i)],
                Slot::Var(i) => {
                    vars[usize::from(i)] = 3.0;
                    3.0
                }
                _ => 0.0,
            },
            &mut || 0.25,
        )
    }

    #[test]
    fn operators_have_their_usual_precedence() {
        for (text, want) in [
            ("1 + 2 * 3", 7.0),
            ("(1 + 2) * 3", 9.0),
            ("10 - 4 - 3", 3.0),
            ("2 * 3 % 4", 2.0),
            ("-2 * -3", 6.0),
            ("- (1 + 2)", -3.0),
            ("1 < 2 && 2 < 3", 1.0),
            ("1 > 2 || 2 > 3", 0.0),
            ("1 + 1 == 2", 1.0),
            ("1 == 1 && 0", 0.0),
            ("!0 + 1", 2.0),
            ("!(1 > 2)", 1.0),
            ("2 * x - y", 12.0),
            ("x / y / 2", 1.75),
            ("7 % 4 + 1", 4.0),
            ("1e1 + .5 + 2.5f", 13.0),
            ("true + false + 2", 3.0),
        ] {
            assert!(
                (run(text) - want).abs() < 1e-5,
                "{text} = {} not {want}",
                run(text)
            );
        }
    }

    #[test]
    fn functions_give_their_values() {
        for (text, want) in [
            ("sin(0)", 0.0),
            ("cos(0)", 1.0),
            ("cos(pi)", -1.0),
            ("torad(180)", std::f32::consts::PI),
            ("todeg(pi)", 180.0),
            ("atan2(1, 1)", std::f32::consts::FRAC_PI_4),
            ("asin(1)", std::f32::consts::FRAC_PI_2),
            ("min(3, 1, 2)", 1.0),
            ("max(3, 1, 2)", 3.0),
            ("clamp(5, 0, 2)", 2.0),
            ("clamp(-5, 0, 2)", 0.0),
            ("abs(-3)", 3.0),
            ("floor(2.7)", 2.0),
            ("ceil(2.1)", 3.0),
            ("frac(2.75)", 0.75),
            ("frac(-0.25)", 0.75),
            ("round(2.5)", 3.0),
            ("signum(-4)", -1.0),
            ("sqrt(16)", 4.0),
            ("pow(2, 10)", 1024.0),
            ("exp(0)", 1.0),
            ("log(1)", 0.0),
            ("fmod(7, 4)", 3.0),
            ("fmod(-1, 4)", 3.0),
            ("lerp(0.25, 10, 20)", 12.5),
            ("between(5, 1, 9)", 1.0),
            ("between(0, 1, 9)", 0.0),
            ("equals(1, 1.05, 0.1)", 1.0),
            ("equals(1, 2, 0.1)", 0.0),
            ("in(3, 1, 2, 3)", 1.0),
            ("in(4, 1, 2, 3)", 0.0),
            ("random()", 0.25),
            ("print(3 + 1)", 4.0),
        ] {
            assert!(
                (run(text) - want).abs() < 1e-5,
                "{text} = {} not {want}",
                run(text)
            );
        }
    }

    #[test]
    fn if_and_ifb_choose_and_chain() {
        for (text, want) in [
            ("if(1, 10, 20)", 10.0),
            ("if(0, 10, 20)", 20.0),
            ("ifb(x > 5, 1, 0)", 1.0),
            ("if(x > 100, 1, x > 5, 2, 3)", 2.0),
            ("if(x > 100, 1, x > 50, 2, 3)", 3.0),
            // Nested, with other functions inside.
            ("if(flag, sin(torad(90)) * 4, 0) + 1", 5.0),
            ("max(if(x > 1, y, 9), min(1, if(0, 5, 3)))", 2.0),
            ("clamp(lerp(t, 0, x * 2), 0, 10)", 7.0),
        ] {
            assert!(
                (run(text) - want).abs() < 1e-5,
                "{text} = {} not {want}",
                run(text)
            );
        }
    }

    #[test]
    fn constants_are_folded_and_variables_are_not() {
        let program = |text: &str| Program::parse(text, &mut Test, &mut |_| {}).unwrap();
        assert_eq!(program("1 + 2 * 3").constant(), Some(7.0));
        assert_eq!(program("sin(0) + cos(0) * torad(180)").ops().len(), 1);
        assert_eq!(program("if(1 > 0, 4, 5) * 2").constant(), Some(8.0));
        // A variable stops folding, but what's around it still folds.
        let p = program("x * (2 + 3)");
        assert_eq!(p.constant(), None);
        assert_eq!(
            p.ops(),
            [Op::Load(Slot::Entity(0)), Op::Const(5.0), Op::Mul]
        );
        // random() is not pure: never folded.
        assert_eq!(program("random()").constant(), None);
    }

    #[test]
    fn unknown_names_read_as_zero_and_are_reported() {
        let mut seen = Vec::new();
        let program = Program::parse("mystery + 2 * other", &mut Test, &mut |n| {
            seen.push(n.to_owned())
        })
        .unwrap();
        assert_eq!(seen, ["mystery", "other"]);
        assert_eq!(program.constant(), Some(0.0));
    }

    #[test]
    fn bad_expressions_are_errors_not_panics() {
        for text in [
            "",
            "1 +",
            "(1 + 2",
            "1 + 2)",
            "sin()",
            "sin(1, 2)",
            "if(1, 2)",
            "nosuch(1)",
            "1 $ 2",
            "min(1)",
            "((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((((1))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))))",
        ] {
            assert!(
                Program::parse(text, &mut Test, &mut |_| {}).is_err(),
                "{text:?} should not parse"
            );
        }
        // Division by zero is 0, not infinity.
        assert_eq!(run("1 / 0"), 0.0);
        assert_eq!(run("5 % 0"), 0.0);
    }

    #[test]
    fn a_variable_a_model_keeps_is_read_each_time() {
        let program = Program::parse("var.n * 2 + 1", &mut Test, &mut |_| {}).unwrap();
        let mut kept = 3.0;
        let mut read = |slot| if slot == Slot::Var(0) { kept } else { 0.0 };
        assert_eq!(program.eval(&mut read, &mut || 0.0), 7.0);
        kept = 10.0;
        let mut read = |slot| if slot == Slot::Var(0) { kept } else { 0.0 };
        assert_eq!(program.eval(&mut read, &mut || 0.0), 21.0);
    }

    /// Evaluates `text` with a random() that counts up, as frames go by.
    fn run_with_counter(text: &str, counter: &mut f32) -> f32 {
        let program = Program::parse(text, &mut Test, &mut |_| {}).unwrap();
        program.eval(
            &mut |slot| match slot {
                Slot::Entity(i) => [7.0, 2.0, 0.5, 1.0][usize::from(i)],
                _ => 0.0,
            },
            &mut || {
                *counter += 0.01;
                *counter
            },
        )
    }

    #[test]
    fn random_with_a_seed_is_the_same_every_frame_and_differs_by_seed() {
        let mut counter = 0.0;
        let first = run_with_counter("random(x)", &mut counter);
        for _ in 0..50 {
            assert_eq!(run_with_counter("random(x)", &mut counter), first);
        }
        assert_eq!(
            counter, 0.0,
            "a seeded random never draws from the sequence"
        );
        assert!((0.0..1.0).contains(&first));
        let other = run_with_counter("random(y)", &mut counter);
        assert_ne!(first, other);
        // Every integer seed stays in 0..1 and they're spread out.
        let mut seen = std::collections::HashSet::new();
        for seed in 0..1000 {
            let v = seeded_random(seed as f32);
            assert!((0.0..1.0).contains(&v));
            seen.insert(v.to_bits());
        }
        assert_eq!(seen.len(), 1000);
        // With no argument it still changes frame to frame.
        let a = run_with_counter("random()", &mut counter);
        let b = run_with_counter("random()", &mut counter);
        assert_ne!(a, b);
    }

    #[test]
    fn a_seeded_wobble_moves_smoothly() {
        let program = Program::parse("sin(t * (1 + random(x)))", &mut Test, &mut |_| {}).unwrap();
        let at = |t: f32| {
            program.eval(
                &mut |slot| match slot {
                    Slot::Entity(0) => 7.0,
                    Slot::Entity(2) => t,
                    _ => 0.0,
                },
                &mut || panic!("no draw wanted"),
            )
        };
        let mut last = at(0.0);
        for step in 1..200 {
            let now = at(step as f32 * 0.01);
            assert!((now - last).abs() < 0.05, "jumped at step {step}");
            last = now;
        }
    }
}
