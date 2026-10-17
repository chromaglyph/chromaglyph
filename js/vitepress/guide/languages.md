# Seven languages

The seven blocks on this page are batched into **one font** per render
(cross-language global disambiguation). Every block copies out as plain
text.

## Rust

String escapes: `\` plus any single glyph advances as a two-glyph rule;
`3.14` uses a lookahead rule and the dot in `1.max` is not swallowed.

```rust
fn main() {
    let path = "src\\lib.rs";
    let msg = format!("escaped: \"{}\" \n tab:\t {}", path, 42);
    let version = 0.2;   // numeric state: digits on both sides color together
    let tuple = (1.max(2), 3.14f64, 'x');
    println!("{msg} v{version} {:?}", tuple);
}
```

## TypeScript

Template interpolation matches the editor: string segments orange, `${`/`}`
keep the string color, the expression renders uncolored, and the string
resumes after the closer.

```ts
interface User {
  readonly id: number
  name: string
  tags?: string[]
}

function greet(user: User): string {
  const { name, tags = [] } = user
  return `hi ${name.toUpperCase()} bye [${tags.join(', ')}]`
}

const greetAliased = greet
export { greet, greetAliased }
```

## JavaScript

Three quote kinds are independent states: inside `'a"b'` the double quote
is string body.

```js
const single = 'he said "stop"'
const double = "it's fine"
const backtick = `sum is ${1 + 2}`
const $dollar = 'is an identifier character, $return is a name not a keyword'

console.log(single, double, backtick, $dollar)

function demo() {
    const name = 'chromaglyph';
    let tmpl = `one-line template ${name}`;
    let empty = ""; let mixed = 'a"b';
    let q = 'it\'s'; let r = "a\"b";
    if ($use) return 42;
    let multi = `line one,
after the newline`;
}
```

## Python

Triple-quoted strings beat the simple quote (longest first) and never
close each other early.

```python
def normalize(text: str) -> str:
    doc = """triple quoted 'won't close this"""
    other = '''also fine "nested" here'''
    return f"{text.strip()} / {doc[:1]}{other[:1]}"

LIMIT = 100_000
print(normalize("  ok  "), LIMIT)
```

## Go

Backtick raw strings have no escapes: a backslash before the closing
backtick does not suppress it (per-kind `escapes` switch).

```go
package main

import "fmt"

func main() {
	raw := `c:\windows\path` // no escapes inside backticks
	runes := 'x'
	text := "quoted \"inner\""
	fmt.Println(raw, runes, text)
}
```

## Java

`class`/`char` go through declaration/modifier keyword groups and render
in dark-plus' teal `storage.type.java`, exactly like VS Code.

```java
public final class Parser {
    private static final char QUOTE = '"';

    public static boolean isBalanced(String input) {
        int depth = 0;
        for (char c : input.toCharArray()) {
            if (c == '(') depth++;
            else if (c == ')') depth--;
        }
        return depth == 0;
    }
}
```

## C

Preprocessor directive words color purple like an editor's directives —
the `#` glyph is not part of the identifier to the font and stays plain.
`sizeof` is blue, `true`/`false`/`NULL` are language constants.

```c
#include <stdio.h>
#define MAX_STACK 64

typedef struct { int items[MAX_STACK]; int top; } stack_t;

int push(stack_t *s, int value) {
    if (s->top == MAX_STACK || s == NULL) return -1;
    s->items[s->top++] = value;
    return 0; /* block comment closes textually on this line */
}
```
