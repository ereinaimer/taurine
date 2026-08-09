#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,
    OpenParen,
    CloseParen,
    Ident(String),
}

pub fn tokenize(expr: &str) -> Option<Vec<(Token, bool)>> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();
    let mut whitespace_pending = false;

    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                whitespace_pending = true;
                chars.next();
            }
            '+' => {
                tokens.push((Token::Plus, whitespace_pending));
                whitespace_pending = false;
                chars.next();
            }
            '-' => {
                tokens.push((Token::Minus, whitespace_pending));
                whitespace_pending = false;
                chars.next();
            }
            '*' => {
                tokens.push((Token::Star, whitespace_pending));
                whitespace_pending = false;
                chars.next();
            }
            '/' => {
                tokens.push((Token::Slash, whitespace_pending));
                whitespace_pending = false;
                chars.next();
            }
            '%' => {
                tokens.push((Token::Percent, whitespace_pending));
                whitespace_pending = false;
                chars.next();
            }
            '^' => {
                tokens.push((Token::Caret, whitespace_pending));
                whitespace_pending = false;
                chars.next();
            }
            '(' => {
                tokens.push((Token::OpenParen, whitespace_pending));
                whitespace_pending = false;
                chars.next();
            }
            ')' => {
                tokens.push((Token::CloseParen, whitespace_pending));
                whitespace_pending = false;
                chars.next();
            }
            c if c.is_alphabetic() => {
                let mut ident = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphabetic() {
                        ident.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                match ident.to_lowercase().as_str() {
                    "pi" => tokens.push((Token::Number(std::f64::consts::PI), whitespace_pending)),
                    "e" => tokens.push((Token::Number(std::f64::consts::E), whitespace_pending)),
                    name => tokens.push((Token::Ident(name.to_string()), whitespace_pending)),
                }
                whitespace_pending = false;
            }
            c if c.is_ascii_digit() || c == '.' => {
                let mut num_str = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '.' {
                        num_str.push(ch);
                        chars.next();
                    } else if ch == 'e' || ch == 'E' {
                        // Check if it's scientific notation: e[+-]?digit
                        // We need to look ahead. Since we only have one peek,
                        // we'll just consume it and if parsing fails, we fail the token.
                        num_str.push(ch);
                        chars.next();
                        if let Some(&next) = chars.peek()
                            && (next == '+' || next == '-')
                        {
                            num_str.push(next);
                            chars.next();
                        }
                        // We expect at least one digit here
                        while let Some(&next_digit) = chars.peek() {
                            if next_digit.is_ascii_digit() {
                                num_str.push(next_digit);
                                chars.next();
                            } else {
                                break;
                            }
                        }
                    } else {
                        break;
                    }
                }
                if let Ok(num) = num_str.parse::<f64>() {
                    tokens.push((Token::Number(num), whitespace_pending));
                    whitespace_pending = false;
                } else {
                    return None;
                }
            }
            _ => return None,
        }
    }

    if tokens.is_empty() {
        return None;
    }

    Some(tokens)
}

pub fn parse_expression(tokens: &[(Token, bool)]) -> Option<f64> {
    let mut pos = 0;
    let res = parse_add_sub(tokens, &mut pos)?;

    if pos != tokens.len() {
        return None;
    }

    Some(res)
}

fn parse_add_sub(tokens: &[(Token, bool)], pos: &mut usize) -> Option<f64> {
    let mut left = parse_mul_div(tokens, pos)?;

    while *pos < tokens.len() {
        match &tokens[*pos] {
            (Token::Plus, _) => {
                *pos += 1;
                let right = parse_mul_div(tokens, pos)?;
                left += right;
            }
            (Token::Minus, _) => {
                *pos += 1;
                let right = parse_mul_div(tokens, pos)?;
                left -= right;
            }
            _ => break,
        }
    }

    Some(left)
}

fn parse_mul_div(tokens: &[(Token, bool)], pos: &mut usize) -> Option<f64> {
    let mut left = parse_unary(tokens, pos)?;

    while *pos < tokens.len() {
        match &tokens[*pos] {
            (Token::Star, _) => {
                *pos += 1;
                let right = parse_unary(tokens, pos)?;
                left *= right;
            }
            (Token::Slash, _) => {
                *pos += 1;
                let right = parse_unary(tokens, pos)?;
                left /= right;
            }
            (Token::Percent, _) => {
                *pos += 1;
                let right = parse_unary(tokens, pos)?;
                left %= right;
            }
            // Implicit Multiplication: number followed by '(', 'Number', or 'Ident'
            // only when the right operand is directly adjacent (no whitespace before it).
            (Token::OpenParen | Token::Number(_) | Token::Ident(_), false) => {
                let right = parse_unary(tokens, pos)?;
                left *= right;
            }
            _ => break,
        }
    }

    Some(left)
}

fn parse_unary(tokens: &[(Token, bool)], pos: &mut usize) -> Option<f64> {
    if *pos < tokens.len() {
        match &tokens[*pos] {
            (Token::Minus, _) => {
                *pos += 1;
                let val = parse_unary(tokens, pos)?;
                return Some(-val);
            }
            (Token::Plus, _) => {
                *pos += 1;
                return parse_unary(tokens, pos);
            }
            _ => {}
        }
    }
    parse_exponent(tokens, pos)
}

fn parse_exponent(tokens: &[(Token, bool)], pos: &mut usize) -> Option<f64> {
    let left = parse_primary(tokens, pos)?;

    if *pos < tokens.len() && tokens[*pos].0 == Token::Caret {
        *pos += 1;
        // Exponents are right-associative: 2^3^2 = 2^(3^2)
        let right = parse_exponent(tokens, pos)?;
        return Some(left.powf(right));
    }

    Some(left)
}

fn parse_primary(tokens: &[(Token, bool)], pos: &mut usize) -> Option<f64> {
    if *pos >= tokens.len() {
        return None;
    }

    match &tokens[*pos] {
        (Token::Number(n), _) => {
            *pos += 1;
            Some(*n)
        }
        (Token::Ident(name), _) => {
            *pos += 1;
            if *pos < tokens.len() && tokens[*pos].0 == Token::OpenParen {
                *pos += 1;
                let arg = parse_add_sub(tokens, pos)?;
                if *pos < tokens.len() && tokens[*pos].0 == Token::CloseParen {
                    *pos += 1;
                    match name.as_str() {
                        "sqrt" => Some(arg.sqrt()),
                        "abs" => Some(arg.abs()),
                        "floor" => Some(arg.floor()),
                        "ceil" => Some(arg.ceil()),
                        "round" => Some(arg.round()),
                        _ => None,
                    }
                } else {
                    None
                }
            } else {
                None
            }
        }
        (Token::OpenParen, _) => {
            *pos += 1;
            let val = parse_add_sub(tokens, pos)?;
            if *pos < tokens.len() && tokens[*pos].0 == Token::CloseParen {
                *pos += 1;
                Some(val)
            } else {
                None
            }
        }
        _ => None,
    }
}

pub fn is_single_operand(tokens: &[(Token, bool)]) -> bool {
    let mut operand_count = 0;
    for token in tokens {
        match token {
            (Token::Number(_), _) | (Token::Ident(_), _) => {
                operand_count += 1;
            }
            (Token::OpenParen, _) | (Token::CloseParen, _) => {}
            _ => {
                return false;
            }
        }
    }
    operand_count == 1
}
