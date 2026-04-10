#[derive(Debug, Clone, PartialEq)]
pub enum Token {
    Number(f64),
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    OpenParen,
    CloseParen,
}

pub fn tokenize(expr: &str) -> Option<Vec<Token>> {
    let mut tokens = Vec::new();
    let mut chars = expr.chars().peekable();

    while let Some(&c) = chars.peek() {
        match c {
            ' ' | '\t' | '\r' | '\n' => {
                chars.next();
            }
            '+' => {
                tokens.push(Token::Plus);
                chars.next();
            }
            '-' => {
                tokens.push(Token::Minus);
                chars.next();
            }
            '*' => {
                tokens.push(Token::Star);
                chars.next();
            }
            '/' => {
                tokens.push(Token::Slash);
                chars.next();
            }
            '%' => {
                tokens.push(Token::Percent);
                chars.next();
            }
            '(' => {
                tokens.push(Token::OpenParen);
                chars.next();
            }
            ')' => {
                tokens.push(Token::CloseParen);
                chars.next();
            }
            'p' | 'e' | 'P' | 'E' => {
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
                    "pi" => tokens.push(Token::Number(std::f64::consts::PI)),
                    "e" => tokens.push(Token::Number(std::f64::consts::E)),
                    _ => return None,
                }
            }
            c if c.is_ascii_digit() || c == '.' => {
                let mut num_str = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '.' {
                        num_str.push(ch);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Ok(num) = num_str.parse::<f64>() {
                    tokens.push(Token::Number(num));
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

pub fn parse_expression(tokens: &[Token]) -> Option<f64> {
    let mut pos = 0;
    let res = parse_add_sub(tokens, &mut pos)?;

    if pos != tokens.len() {
        return None;
    }

    Some(res)
}

fn parse_add_sub(tokens: &[Token], pos: &mut usize) -> Option<f64> {
    let mut left = parse_mul_div(tokens, pos)?;

    while *pos < tokens.len() {
        match tokens[*pos] {
            Token::Plus => {
                *pos += 1;
                let right = parse_mul_div(tokens, pos)?;
                left += right;
            }
            Token::Minus => {
                *pos += 1;
                let right = parse_mul_div(tokens, pos)?;
                left -= right;
            }
            _ => break,
        }
    }

    Some(left)
}

fn parse_mul_div(tokens: &[Token], pos: &mut usize) -> Option<f64> {
    let mut left = parse_primary(tokens, pos)?;

    while *pos < tokens.len() {
        match tokens[*pos] {
            Token::Star => {
                *pos += 1;
                let right = parse_primary(tokens, pos)?;
                left *= right;
            }
            Token::Slash => {
                *pos += 1;
                let right = parse_primary(tokens, pos)?;
                left /= right;
            }
            Token::Percent => {
                *pos += 1;
                let right = parse_primary(tokens, pos)?;
                left %= right;
            }
            _ => break,
        }
    }

    Some(left)
}

fn parse_primary(tokens: &[Token], pos: &mut usize) -> Option<f64> {
    if *pos >= tokens.len() {
        return None;
    }

    match &tokens[*pos] {
        Token::Number(n) => {
            *pos += 1;
            Some(*n)
        }
        Token::Minus => {
            *pos += 1;
            let val = parse_primary(tokens, pos)?;
            Some(-val)
        }
        Token::Plus => {
            *pos += 1;
            parse_primary(tokens, pos)
        }
        Token::OpenParen => {
            *pos += 1;
            let val = parse_add_sub(tokens, pos)?;
            if *pos < tokens.len() && tokens[*pos] == Token::CloseParen {
                *pos += 1;
                Some(val)
            } else {
                None
            }
        }
        _ => None,
    }
}
