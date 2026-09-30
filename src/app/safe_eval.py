# -*- coding: utf-8 -*-
"""Avaliador restrito de expressões matemáticas das fórmulas do usuário.

Substitui o uso de eval(): só aceita números, nomes conhecidos, operadores
aritméticos/comparação, if-else inline, chamadas a funções permitidas e
atributos públicos do módulo math. Qualquer outra construção gera ValueError.
Nomes desconhecidos geram NameError (usado para resolver dependências).
"""
import ast
import math
import operator

_BIN_OPS = {
    ast.Add: operator.add,
    ast.Sub: operator.sub,
    ast.Mult: operator.mul,
    ast.Div: operator.truediv,
    ast.FloorDiv: operator.floordiv,
    ast.Mod: operator.mod,
    ast.Pow: operator.pow,
}
_UNARY_OPS = {ast.UAdd: operator.pos, ast.USub: operator.neg, ast.Not: operator.not_}
_CMP_OPS = {
    ast.Eq: operator.eq, ast.NotEq: operator.ne,
    ast.Lt: operator.lt, ast.LtE: operator.le,
    ast.Gt: operator.gt, ast.GtE: operator.ge,
}
_MAX_POW_EXPONENT = 1000


def _eval(node, names):
    if isinstance(node, ast.Expression):
        return _eval(node.body, names)
    if isinstance(node, ast.Constant):
        if isinstance(node.value, (int, float, bool)):
            return node.value
        raise ValueError(f"constante não permitida: {node.value!r}")
    if isinstance(node, ast.Name):
        if node.id not in names:
            raise NameError(f"name '{node.id}' is not defined")
        return names[node.id]
    if isinstance(node, ast.Attribute):
        base = _eval(node.value, names)
        if base is not math or node.attr.startswith('_'):
            raise ValueError(f"atributo não permitido: {node.attr}")
        return getattr(math, node.attr)
    if isinstance(node, ast.BinOp) and type(node.op) in _BIN_OPS:
        left = _eval(node.left, names)
        right = _eval(node.right, names)
        if isinstance(node.op, ast.Pow) and abs(right) > _MAX_POW_EXPONENT:
            raise ValueError("expoente muito grande")
        return _BIN_OPS[type(node.op)](left, right)
    if isinstance(node, ast.UnaryOp) and type(node.op) in _UNARY_OPS:
        return _UNARY_OPS[type(node.op)](_eval(node.operand, names))
    if isinstance(node, ast.BoolOp):
        values = [_eval(v, names) for v in node.values]
        if isinstance(node.op, ast.And):
            return all(values)
        return any(values)
    if isinstance(node, ast.Compare):
        left = _eval(node.left, names)
        for op, comp in zip(node.ops, node.comparators):
            if type(op) not in _CMP_OPS:
                raise ValueError("comparação não permitida")
            right = _eval(comp, names)
            if not _CMP_OPS[type(op)](left, right):
                return False
            left = right
        return True
    if isinstance(node, ast.IfExp):
        return _eval(node.body, names) if _eval(node.test, names) else _eval(node.orelse, names)
    if isinstance(node, ast.Call):
        if node.keywords:
            raise ValueError("argumentos nomeados não permitidos")
        func = _eval(node.func, names)
        if not callable(func) or isinstance(func, type(math)):
            raise ValueError("chamada não permitida")
        return func(*[_eval(a, names) for a in node.args])
    raise ValueError(f"expressão não permitida: {type(node).__name__}")


def safe_eval(expr: str, names: dict):
    """Avalia `expr` usando apenas os nomes em `names`."""
    tree = ast.parse(str(expr).strip(), mode='eval')
    return _eval(tree, names)
