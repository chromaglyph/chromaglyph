# Languages

The eleven blocks on this page are batched into **one font** per render
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

const a = 0;
await abc();

const fn = () => {
  return {
    msg: 'message'
  }
}

export default {
  data () {
    return {
      msg: 'am'
    }
  }
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

## C++

Keywords may carry digits: `char8_t` colors as one word (the digit glyph
joins the keyword's class and still opens numbers elsewhere). Raw
strings `R"(...)"` and digit separators `1'000` are deliberately not
modeled — a glyph FSM cannot know a user-chosen raw delimiter.

```cpp
#include <vector>

namespace demo {
template <typename T> class Stack {
  public:
    void push(const T &value) { items.emplace_back(value); }
    auto size() const noexcept { return items.size(); }
  private:
    std::vector<T> items;
};
}

int run(demo::Stack<int> &s) {
    constexpr int LIMIT = 64;
    auto total = s.size() + LIMIT;
    if (total == 0) return -1;
    delete nullptr; // never reached — nullptr is a constant, delete blue
    return sizeof(int) * 2;
}
```

## C#

Verbatim strings `@"..."` color as strings with the backslash taken
literally (the two-char opener beats the plain quote at the same
position); verbatim identifiers like `@return` stay uncolored.
Contextual keywords (`value`, `when`, `where`) are deliberately absent —
a stateless in-font lexer cannot tell them from ordinary names.

```csharp
using System.Collections.Generic;

namespace Demo {
public class Stack<T> {
    private readonly List<T> items = new();

    public void Push(T value) => items.Add(value);
    public int Count => items.Count;

    public static string Describe(string path) {
        var raw = @"C:\temp\";           // verbatim: backslashes are literal
        var msg = $"loaded {path} from {raw}";
        return msg; // interpolated colors as an ordinary string
    }
}
}
```

## HTML

The one non-keyword language: a markup **tag state machine** walks
`name → attribute names → = → values` through glyph substitutions.
Delimiters (`< > / =` quotes) stay plain — they are uncolored state
carriers. `<3` and a spaced `< b` stay plain (the opener requires an
identifier lookahead), entities color in text only, and an unclosed
tag dies at its line's end like every state here.

```html
<div class="hero">
  <!-- tag machine: names blue, attrs light, values orange -->
  <a href="https://example.io/x?a=1&amp;b=2">a link</a> &amp; plain text
  <img src='logo.png' alt="logo" />
</div>
```

## CSS

Selectors, properties and values: a property name colors only when a
`:` follows it (the ~365-name standard list), so `a:hover` stays a
selector; values ride uncolored until `;` with numbers, strings and
comments colored normally. Multi-line rule blocks rely on the colon
lookahead — block state cannot cross a line.

```css
.card > .title {
  /* properties need the colon */
  margin: 0 auto;
  padding: 4px 8px; /* unquoted value */
  font: 12px/1.5 sans-serif;
  background: url("bg.png") no-repeat;
}
@media (min-width: 600px) { .card { padding: 8px !important; } }
```
