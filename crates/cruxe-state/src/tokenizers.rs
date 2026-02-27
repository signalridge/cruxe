use tantivy::tokenizer::{BoxTokenStream, Token, TokenStream, Tokenizer};

/// CamelCase tokenizer: splits `CamelCaseName` → `[camel, case, name]`
#[derive(Clone)]
pub struct CodeCamelTokenizer;

impl Tokenizer for CodeCamelTokenizer {
    type TokenStream<'a> = BoxTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let tokens = split_camel_case(text);
        BoxTokenStream::new(VecTokenStream::new(tokens))
    }
}

/// snake_case tokenizer: splits `snake_case_name` → `[snake, case, name]`
#[derive(Clone)]
pub struct CodeSnakeTokenizer;

impl Tokenizer for CodeSnakeTokenizer {
    type TokenStream<'a> = BoxTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let tokens = split_snake_case(text);
        BoxTokenStream::new(VecTokenStream::new(tokens))
    }
}

/// Dotted name tokenizer: splits `pkg.module.Class` → `[pkg, module, class]`
#[derive(Clone)]
pub struct CodeDottedTokenizer;

impl Tokenizer for CodeDottedTokenizer {
    type TokenStream<'a> = BoxTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let tokens = split_dotted(text);
        BoxTokenStream::new(VecTokenStream::new(tokens))
    }
}

/// Signature tokenizer: combines camel + snake splitting for comprehensive search.
/// `fn validateToken(snake_arg: Type)` → camel tokens + snake tokens (deduplicated).
#[derive(Clone)]
pub struct CodeSignatureTokenizer;

impl Tokenizer for CodeSignatureTokenizer {
    type TokenStream<'a> = BoxTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let mut seen = std::collections::HashSet::new();
        let mut tokens = Vec::new();
        for t in split_camel_case(text) {
            if seen.insert(t.clone()) {
                tokens.push(t);
            }
        }
        for t in split_snake_case(text) {
            if seen.insert(t.clone()) {
                tokens.push(t);
            }
        }
        BoxTokenStream::new(VecTokenStream::new(tokens))
    }
}

/// File path tokenizer: splits `src/auth/handler.rs` → `[src, auth, handler, rs]`
#[derive(Clone)]
pub struct CodePathTokenizer;

impl Tokenizer for CodePathTokenizer {
    type TokenStream<'a> = BoxTokenStream<'a>;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> Self::TokenStream<'a> {
        let tokens = split_path(text);
        BoxTokenStream::new(VecTokenStream::new(tokens))
    }
}

fn split_camel_case(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();

    for ch in text.chars() {
        if ch.is_uppercase() && !current.is_empty() {
            tokens.push(current.to_lowercase());
            current.clear();
        }
        if ch.is_alphanumeric() {
            current.push(ch);
        } else if !current.is_empty() {
            tokens.push(current.to_lowercase());
            current.clear();
        }
    }
    if !current.is_empty() {
        tokens.push(current.to_lowercase());
    }
    tokens
}

fn split_snake_case(text: &str) -> Vec<String> {
    text.split(|c: char| c == '_' || c == '-' || !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

fn split_dotted(text: &str) -> Vec<String> {
    text.split(['.', ':'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

fn split_path(text: &str) -> Vec<String> {
    text.split(['/', '\\', '.'])
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// A simple token stream backed by a Vec.
struct VecTokenStream {
    tokens: Vec<Token>,
    index: usize,
}

impl VecTokenStream {
    fn new(strings: Vec<String>) -> Self {
        let mut offset = 0;
        let tokens = strings
            .into_iter()
            .enumerate()
            .map(|(pos, text)| {
                let from = offset;
                offset += text.len();
                Token {
                    offset_from: from,
                    offset_to: offset,
                    position: pos,
                    text,
                    position_length: 1,
                }
            })
            .collect();
        Self { tokens, index: 0 }
    }
}

impl TokenStream for VecTokenStream {
    fn advance(&mut self) -> bool {
        if self.index < self.tokens.len() {
            self.index += 1;
            true
        } else {
            false
        }
    }

    fn token(&self) -> &Token {
        &self.tokens[self.index - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.index - 1]
    }
}

/// Register all custom tokenizers on a Tantivy index.
pub fn register_tokenizers(manager: &tantivy::tokenizer::TokenizerManager) {
    manager.register("code_camel", CodeCamelTokenizer);
    manager.register("code_snake", CodeSnakeTokenizer);
    manager.register("code_dotted", CodeDottedTokenizer);
    manager.register("code_path", CodePathTokenizer);
    manager.register("code_signature", CodeSignatureTokenizer);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tokenize(tokenizer: &mut impl Tokenizer, text: &str) -> Vec<String> {
        let mut stream = tokenizer.token_stream(text);
        let mut result = Vec::new();
        while stream.advance() {
            result.push(stream.token().text.clone());
        }
        result
    }

    #[test]
    fn test_camel_case() {
        let mut t = CodeCamelTokenizer;
        assert_eq!(tokenize(&mut t, "CamelCase"), vec!["camel", "case"]);
        assert_eq!(
            tokenize(&mut t, "CamelCaseName"),
            vec!["camel", "case", "name"]
        );
        assert_eq!(
            tokenize(&mut t, "getHTTPResponse"),
            vec!["get", "h", "t", "t", "p", "response"]
        );
        assert_eq!(tokenize(&mut t, "simple"), vec!["simple"]);
    }

    #[test]
    fn test_snake_case() {
        let mut t = CodeSnakeTokenizer;
        assert_eq!(tokenize(&mut t, "snake_case"), vec!["snake", "case"]);
        assert_eq!(
            tokenize(&mut t, "snake_case_name"),
            vec!["snake", "case", "name"]
        );
        assert_eq!(tokenize(&mut t, "kebab-case"), vec!["kebab", "case"]);
    }

    #[test]
    fn test_dotted() {
        let mut t = CodeDottedTokenizer;
        assert_eq!(
            tokenize(&mut t, "pkg.module.Class"),
            vec!["pkg", "module", "class"]
        );
        assert_eq!(
            tokenize(&mut t, "std::io::Error"),
            vec!["std", "io", "error"]
        );
    }

    #[test]
    fn test_path() {
        let mut t = CodePathTokenizer;
        assert_eq!(
            tokenize(&mut t, "src/auth/handler.rs"),
            vec!["src", "auth", "handler", "rs"]
        );
        assert_eq!(
            tokenize(&mut t, "crates/core/lib.rs"),
            vec!["crates", "core", "lib", "rs"]
        );
    }

    #[test]
    fn test_signature_tokenizer_combines_camel_and_snake() {
        let mut t = CodeSignatureTokenizer;
        // A mixed signature: camelCase function with snake_case param
        let tokens = tokenize(&mut t, "fn validateToken(user_id: String)");
        // Should contain tokens from both camel and snake splitting
        assert!(tokens.contains(&"validate".to_string()));
        assert!(tokens.contains(&"token".to_string()));
        assert!(tokens.contains(&"user".to_string()));
        assert!(tokens.contains(&"id".to_string()));
        assert!(tokens.contains(&"string".to_string()));
    }
}
