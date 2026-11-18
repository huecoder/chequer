
pub fn evaluate(text: &str) -> Option<f64> {
    let mut parser = Parser { chars: text.chars().filter(|c| !c.is_whitespace()).collect(), position: 0 };
    let value = parser.sum()?;
    (parser.position == parser.chars.len() && value.is_finite()).then_some(value)
}

struct Parser {
    chars: Vec<char>,
    position: usize,
}

impl Parser {
    fn eat(&mut self, accepted: &[char]) -> Option<char> {
        let next = *self.chars.get(self.position).filter(|c| accepted.contains(c))?;
        self.position += 1;
        Some(next)
    }

    fn sum(&mut self) -> Option<f64> {
        let mut total = self.product()?;
        while let Some(sign) = self.eat(&['+', '-', '−']) {
            let term = self.product()?;
            total = if sign == '+' { total + term } else { total - term };
        }
        Some(total)
    }

    fn product(&mut self) -> Option<f64> {
        let mut total = self.unary()?;
        while let Some(sign) = self.eat(&['*', '/', '×', '÷', ':']) {
            let factor = self.unary()?;
            total = if matches!(sign, '*' | '×') { total * factor } else { total / factor };
        }
        Some(total)
    }

    fn unary(&mut self) -> Option<f64> {
        match self.eat(&['-', '−', '+']) {
            Some('+') => self.unary(),
            Some(_) => self.unary().map(|value| -value),
            None => self.power(),
        }
    }

    fn power(&mut self) -> Option<f64> {
        let base = self.atom()?;
        if self.eat(&['^']).is_some() {
            return Some(base.powf(self.unary()?));
        }
        Some(base)
    }

    fn atom(&mut self) -> Option<f64> {
        if self.eat(&['(']).is_some() {
            let inner = self.sum()?;
            self.eat(&[')'])?;
            return Some(inner);
        }
        let start = self.position;
        while self.chars.get(self.position).is_some_and(|c| c.is_ascii_digit() || matches!(c, '.' | ',')) {
            self.position += 1;
        }
        let number: String = self.chars[start..self.position].iter().map(|&c| if c == ',' { '.' } else { c }).collect();
        number.parse().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::evaluate;

    #[test]
    fn evaluates_the_usual_operators_with_precedence() {
        assert_eq!(evaluate("64*2"), Some(128.0));
        assert_eq!(evaluate("2^4"), Some(16.0));
        assert_eq!(evaluate("1024/2"), Some(512.0));
        assert_eq!(evaluate("2+3*4"), Some(14.0));
        assert_eq!(evaluate("(2+3)*4"), Some(20.0));
        assert_eq!(evaluate("-2^2"), Some(-4.0));
        assert_eq!(evaluate("2^3^2"), Some(512.0));
        assert_eq!(evaluate("2 ^ -1"), Some(0.5));
    }

    #[test]
    fn accepts_a_decimal_comma_and_a_plain_number() {
        assert_eq!(evaluate("1,5*2"), Some(3.0));
        assert_eq!(evaluate("42"), Some(42.0));
        assert_eq!(evaluate("1 024"), Some(1024.0));
    }

    #[test]
    fn rejects_garbage_and_non_finite_results() {
        for text in ["", "abc", "2+", "(2", "2)", "1/0", "1.2.3"] {
            assert_eq!(evaluate(text), None, "{text:?}");
        }
    }
}
