//! Lexer das fórmulas: números, identificadores (com `.`) e os operadores
//! aritméticos do Python. Qualquer outro caractere é [`FormulaError::Parse`].

use super::FormulaError;

/// Token da gramática aritmética.
#[derive(Clone, Debug, PartialEq)]
pub(super) enum Token {
    Num(f64),
    /// Identificador, possivelmente com pontos (`math.pi`).
    Ident(String),
    Plus,
    Minus,
    Star,
    Slash,
    DoubleSlash,
    Percent,
    DoubleStar,
    LParen,
    RParen,
    Comma,
}

impl Token {
    /// Descrição para mensagens de erro.
    pub(super) fn describe(&self) -> String {
        match self {
            Token::Num(v) => format!("número {v}"),
            Token::Ident(name) => format!("nome '{name}'"),
            Token::Plus => "'+'".to_string(),
            Token::Minus => "'-'".to_string(),
            Token::Star => "'*'".to_string(),
            Token::Slash => "'/'".to_string(),
            Token::DoubleSlash => "'//'".to_string(),
            Token::Percent => "'%'".to_string(),
            Token::DoubleStar => "'**'".to_string(),
            Token::LParen => "'('".to_string(),
            Token::RParen => "')'".to_string(),
            Token::Comma => "','".to_string(),
        }
    }
}

fn is_ident_start(c: char) -> bool {
    c == '_' || c.is_alphabetic()
}

fn is_ident_continue(c: char) -> bool {
    c == '_' || c.is_alphanumeric()
}

/// Converte `src` em tokens. Espaços em branco (inclusive quebras de linha)
/// são ignorados; a posição nas mensagens conta caracteres a partir de 0.
pub(super) fn tokenize(src: &str) -> Result<Vec<Token>, FormulaError> {
    let chars: Vec<char> = src.chars().collect();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let starts_number = c.is_ascii_digit()
            || (c == '.' && chars.get(i + 1).is_some_and(|d| d.is_ascii_digit()));
        if starts_number {
            let (token, next) = lex_number(&chars, i)?;
            tokens.push(token);
            i = next;
            continue;
        }
        if is_ident_start(c) {
            let (token, next) = lex_ident(&chars, i)?;
            tokens.push(token);
            i = next;
            continue;
        }
        let token = match c {
            '+' => Token::Plus,
            '-' => Token::Minus,
            '%' => Token::Percent,
            '(' => Token::LParen,
            ')' => Token::RParen,
            ',' => Token::Comma,
            '*' if chars.get(i + 1) == Some(&'*') => {
                i += 1;
                Token::DoubleStar
            }
            '*' => Token::Star,
            '/' if chars.get(i + 1) == Some(&'/') => {
                i += 1;
                Token::DoubleSlash
            }
            '/' => Token::Slash,
            other => {
                return Err(FormulaError::Parse(format!(
                    "caractere inválido {other:?} na posição {i}"
                )))
            }
        };
        tokens.push(token);
        i += 1;
    }
    Ok(tokens)
}

/// Consome dígitos (e `_` separadores, descartados) a partir de `*i`,
/// acrescentando-os a `text`; devolve quantos dígitos foram lidos.
fn digits(chars: &[char], i: &mut usize, text: &mut String) -> usize {
    let mut count = 0;
    while *i < chars.len() && (chars[*i].is_ascii_digit() || chars[*i] == '_') {
        if chars[*i] != '_' {
            text.push(chars[*i]);
            count += 1;
        }
        *i += 1;
    }
    count
}

/// Literal numérico do Python: `1`, `1.5`, `5.`, `.5`, `1e-6`, `1.5E3`, `1_000`.
fn lex_number(chars: &[char], start: usize) -> Result<(Token, usize), FormulaError> {
    let mut i = start;
    let mut text = String::new();
    digits(chars, &mut i, &mut text);
    if i < chars.len() && chars[i] == '.' {
        text.push('.');
        i += 1;
        if digits(chars, &mut i, &mut text) == 0 {
            text.push('0');
        }
    }
    if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
        text.push('e');
        i += 1;
        if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
            text.push(chars[i]);
            i += 1;
        }
        if digits(chars, &mut i, &mut text) == 0 {
            return Err(FormulaError::Parse(format!(
                "expoente sem dígitos no literal numérico na posição {start}"
            )));
        }
    }
    text.parse::<f64>()
        .map(|value| (Token::Num(value), i))
        .map_err(|_| FormulaError::Parse(format!("literal numérico inválido {text:?}")))
}

/// Identificador com segmentos separados por `.` (`math.pi`); um `.` precisa
/// ser seguido de outro nome.
fn lex_ident(chars: &[char], start: usize) -> Result<(Token, usize), FormulaError> {
    let mut i = start;
    let mut name = String::new();
    loop {
        while i < chars.len() && is_ident_continue(chars[i]) {
            name.push(chars[i]);
            i += 1;
        }
        if i < chars.len() && chars[i] == '.' {
            if chars.get(i + 1).is_some_and(|&c| is_ident_start(c)) {
                name.push('.');
                i += 1;
                continue;
            }
            return Err(FormulaError::Parse(format!(
                "'.' após '{name}' deve ser seguido de um nome (posição {i})"
            )));
        }
        break;
    }
    Ok((Token::Ident(name), i))
}
