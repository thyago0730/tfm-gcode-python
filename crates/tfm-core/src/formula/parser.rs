//! Parser de precedência (Pratt) para a gramática aritmética do Python.
//!
//! Poderes de ligação (esquerda, direita): `+ -` (1, 2); `* / // %` (3, 4);
//! `**` (6, 5) — associativo à direita porque o lado direito é lido com poder
//! menor que o esquerdo. O operando de um unário é lido com poder 5, então
//! `-2**2 == -(2**2)` e `2**-1 == 2**(-1)`, como no Python.

use super::lexer::{tokenize, Token};
use super::{Expr, FormulaError, Op};

/// Profundidade máxima de aninhamento (parênteses, unários, argumentos);
/// acima disso a fórmula é rejeitada em vez de arriscar a pilha.
const MAX_DEPTH: usize = 256;

/// Poder de ligação do operando de `-x` / `+x`.
const UNARY_BP: u8 = 5;

/// Analisa `src` e devolve a árvore da expressão.
///
/// Erros: expressão vazia, caractere fora da gramática, token inesperado,
/// parêntese sem par, chamada mal formada, aninhamento acima de 256 níveis.
pub fn parse(src: &str) -> Result<Expr, FormulaError> {
    let tokens = tokenize(src)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        depth: 0,
    };
    if parser.peek().is_none() {
        return Err(FormulaError::Parse("expressão vazia".to_string()));
    }
    let expr = parser.parse_expr(0)?;
    match parser.peek() {
        None => Ok(expr),
        Some(token) => Err(FormulaError::Parse(format!(
            "token inesperado {} após o fim da expressão",
            token.describe()
        ))),
    }
}

/// Operador binário: (poder à esquerda, poder à direita, operador).
fn binding_power(token: &Token) -> Option<(u8, u8, Op)> {
    match token {
        Token::Plus => Some((1, 2, Op::Add)),
        Token::Minus => Some((1, 2, Op::Sub)),
        Token::Star => Some((3, 4, Op::Mul)),
        Token::Slash => Some((3, 4, Op::Div)),
        Token::DoubleSlash => Some((3, 4, Op::FloorDiv)),
        Token::Percent => Some((3, 4, Op::Mod)),
        Token::DoubleStar => Some((6, 5, Op::Pow)),
        _ => None,
    }
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    fn next(&mut self) -> Option<Token> {
        let token = self.tokens.get(self.pos).cloned();
        if token.is_some() {
            self.pos += 1;
        }
        token
    }

    fn expect(&mut self, want: Token) -> Result<(), FormulaError> {
        match self.next() {
            Some(ref got) if *got == want => Ok(()),
            Some(got) => Err(FormulaError::Parse(format!(
                "esperado {}, obtido {}",
                want.describe(),
                got.describe()
            ))),
            None => Err(FormulaError::Parse(format!(
                "esperado {}, obtido fim da expressão",
                want.describe()
            ))),
        }
    }

    fn parse_expr(&mut self, min_bp: u8) -> Result<Expr, FormulaError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(FormulaError::Parse(format!(
                "expressão aninhada além de {MAX_DEPTH} níveis"
            )));
        }
        let mut lhs = self.parse_prefix()?;
        while let Some((lbp, rbp, op)) = self.peek().and_then(binding_power) {
            if lbp < min_bp {
                break;
            }
            self.next();
            let rhs = self.parse_expr(rbp)?;
            lhs = Expr::Binary(op, Box::new(lhs), Box::new(rhs));
        }
        self.depth -= 1;
        Ok(lhs)
    }

    fn parse_prefix(&mut self) -> Result<Expr, FormulaError> {
        match self.next() {
            None => Err(FormulaError::Parse(
                "fim inesperado da expressão".to_string(),
            )),
            Some(Token::Num(value)) => Ok(Expr::Num(value)),
            Some(Token::Minus) => {
                let operand = self.parse_expr(UNARY_BP)?;
                Ok(Expr::Unary(Op::Neg, Box::new(operand)))
            }
            Some(Token::Plus) => {
                let operand = self.parse_expr(UNARY_BP)?;
                Ok(Expr::Unary(Op::Pos, Box::new(operand)))
            }
            Some(Token::LParen) => {
                let inner = self.parse_expr(0)?;
                self.expect(Token::RParen)?;
                Ok(inner)
            }
            Some(Token::Ident(name)) => {
                if self.peek() == Some(&Token::LParen) {
                    self.next();
                    let args = self.parse_args(&name)?;
                    Ok(Expr::Call(name, args))
                } else {
                    Ok(Expr::Var(name))
                }
            }
            Some(other) => Err(FormulaError::Parse(format!(
                "token inesperado {}",
                other.describe()
            ))),
        }
    }

    /// Argumentos após o `(` de uma chamada, até o `)`; aceita vírgula final.
    fn parse_args(&mut self, name: &str) -> Result<Vec<Expr>, FormulaError> {
        let mut args = Vec::new();
        if self.peek() == Some(&Token::RParen) {
            self.next();
            return Ok(args);
        }
        loop {
            args.push(self.parse_expr(0)?);
            match self.next() {
                Some(Token::Comma) => {
                    if self.peek() == Some(&Token::RParen) {
                        self.next();
                        return Ok(args);
                    }
                }
                Some(Token::RParen) => return Ok(args),
                Some(other) => {
                    return Err(FormulaError::Parse(format!(
                        "esperado ',' ou ')' nos argumentos de {name}(), obtido {}",
                        other.describe()
                    )))
                }
                None => {
                    return Err(FormulaError::Parse(format!(
                        "chamada de {name}() sem ')' de fechamento"
                    )))
                }
            }
        }
    }
}
