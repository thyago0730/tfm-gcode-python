//! Parser de precedência (Pratt) para a gramática aritmética do Python.
//!
//! Poderes de ligação (esquerda, direita): `+ -` (1, 2); `* / // %` (3, 4);
//! `**` (6, 5) — associativo à direita porque o lado direito é lido com poder
//! menor que o esquerdo. O operando de um unário é lido com poder 5, então
//! `-2**2 == -(2**2)` e `2**-1 == 2**(-1)`, como no Python.
//!
//! Dois limites protegem a pilha, ambos [`MAX_DEPTH`]: a **recursão** do
//! parser (parênteses, unários e argumentos aninhados, como `((((1))))`) e a
//! **altura** da árvore produzida, que cresce um nível por termo em cadeias
//! associativas à esquerda como `1+1+...+1` sem nenhuma recursão no parser —
//! e [`super::eval`] e o `Drop` de [`Expr`] são recursivos nessa altura.

use super::lexer::{tokenize, Token};
use super::{Expr, FormulaError, Op};

/// Limite de recursão do parser e de altura da árvore (níveis de [`Expr`],
/// folha = 1). Acima disso a fórmula é rejeitada com [`FormulaError::Parse`]
/// em vez de arriscar estourar a pilha ao analisar, avaliar ou descartar.
const MAX_DEPTH: usize = 256;

/// Poder de ligação do operando de `-x` / `+x`.
const UNARY_BP: u8 = 5;

/// Subárvore acompanhada da sua altura.
type Node = (Expr, usize);

/// Analisa `src` e devolve a árvore da expressão.
///
/// Erros: expressão vazia, caractere fora da gramática, token inesperado,
/// parêntese sem par, chamada mal formada, aninhamento ou encadeamento acima
/// de [`MAX_DEPTH`] níveis.
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
    let (expr, _height) = parser.parse_expr(0)?;
    match parser.peek() {
        None => Ok(expr),
        Some(token) => Err(FormulaError::Parse(format!(
            "token inesperado {} após o fim da expressão",
            token.describe()
        ))),
    }
}

/// Fecha um nó verificando a altura resultante da árvore.
fn node(expr: Expr, height: usize) -> Result<Node, FormulaError> {
    if height > MAX_DEPTH {
        return Err(FormulaError::Parse(format!(
            "fórmula com mais de {MAX_DEPTH} níveis de operações encadeadas ou aninhadas \
             (limite de profundidade da árvore)"
        )));
    }
    Ok((expr, height))
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
    /// Recursão corrente de `parse_expr` (parênteses, unários, argumentos).
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

    fn parse_expr(&mut self, min_bp: u8) -> Result<Node, FormulaError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            return Err(FormulaError::Parse(format!(
                "expressão aninhada além de {MAX_DEPTH} níveis (parênteses, unários ou chamadas)"
            )));
        }
        let (mut lhs, mut height) = self.parse_prefix()?;
        while let Some((lbp, rbp, op)) = self.peek().and_then(binding_power) {
            if lbp < min_bp {
                break;
            }
            self.next();
            let (rhs, rhs_height) = self.parse_expr(rbp)?;
            // Cadeias à esquerda (`1+1+1...`) crescem um nível por termo aqui,
            // sem recursão: a altura é o que limita.
            (lhs, height) = node(
                Expr::Binary(op, Box::new(lhs), Box::new(rhs)),
                1 + height.max(rhs_height),
            )?;
        }
        self.depth -= 1;
        Ok((lhs, height))
    }

    fn parse_prefix(&mut self) -> Result<Node, FormulaError> {
        match self.next() {
            None => Err(FormulaError::Parse(
                "fim inesperado da expressão".to_string(),
            )),
            Some(Token::Num(value)) => Ok((Expr::Num(value), 1)),
            Some(Token::Minus) => {
                let (operand, height) = self.parse_expr(UNARY_BP)?;
                node(Expr::Unary(Op::Neg, Box::new(operand)), height + 1)
            }
            Some(Token::Plus) => {
                let (operand, height) = self.parse_expr(UNARY_BP)?;
                node(Expr::Unary(Op::Pos, Box::new(operand)), height + 1)
            }
            Some(Token::LParen) => {
                let inner = self.parse_expr(0)?;
                self.expect(Token::RParen)?;
                Ok(inner)
            }
            Some(Token::Ident(name)) => {
                if self.peek() == Some(&Token::LParen) {
                    self.next();
                    let (args, height) = self.parse_args(&name)?;
                    node(Expr::Call(name, args), height + 1)
                } else {
                    Ok((Expr::Var(name), 1))
                }
            }
            Some(other) => Err(FormulaError::Parse(format!(
                "token inesperado {}",
                other.describe()
            ))),
        }
    }

    /// Argumentos após o `(` de uma chamada, até o `)`; aceita vírgula final.
    /// Devolve também a maior altura entre os argumentos (0 se não há).
    fn parse_args(&mut self, name: &str) -> Result<(Vec<Expr>, usize), FormulaError> {
        let mut args = Vec::new();
        let mut height = 0;
        if self.peek() == Some(&Token::RParen) {
            self.next();
            return Ok((args, height));
        }
        loop {
            let (arg, arg_height) = self.parse_expr(0)?;
            args.push(arg);
            height = height.max(arg_height);
            match self.next() {
                Some(Token::Comma) => {
                    if self.peek() == Some(&Token::RParen) {
                        self.next();
                        return Ok((args, height));
                    }
                }
                Some(Token::RParen) => return Ok((args, height)),
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
