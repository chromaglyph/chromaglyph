# 支持的语言

本页的十一个代码块在一次渲染中合并进**同一个字体**（跨语言全局消歧）。每一块复制出来都是纯文本。

## Rust

字符串转义：`\` 加任意一个字形按双字形规则前进；`3.14` 走前瞻规则，`1.max` 里的点不会被吞掉。

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

模板插值与编辑器一致：字符串段橙色，`${` / `}` 保持字符串色，表达式不着色，闭合后字符串色恢复。

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

三种引号是独立状态：`'a"b'` 里的双引号属于字符串体。

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

三引号字符串优先于单引号（最长优先），彼此不会提前闭合。

```python
def normalize(text: str) -> str:
    doc = """triple quoted 'won't close this"""
    other = '''also fine "nested" here'''
    return f"{text.strip()} / {doc[:1]}{other[:1]}"

LIMIT = 100_000
print(normalize("  ok  "), LIMIT)
```

## Go

反引号原始字符串没有转义：闭合反引号前的反斜杠不会压制它（按引号种类的 `escapes` 开关）。

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

`class` / `char` 走声明/修饰符关键字组，渲染为 dark-plus 的青色 `storage.type.java`，与 VS Code 一致。

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

预处理指令词像编辑器一样着紫色——`#` 字形对字体而言不属于标识符，保持原色。`sizeof` 蓝色，`true` / `false` / `NULL` 是语言常量。

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

关键字可以带数字：`char8_t` 整词着色（数字字形并入关键字类，在别处仍能开启数字）。原始字符串 `R"(...)"` 和数字分隔符 `1'000` 刻意不建模——字形状态机无法得知用户自定的原始分隔符。

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

逐字字符串 `@"..."` 按字符串着色，反斜杠按字面处理（双字符开头在同一位置胜过普通引号）；`@return` 这类逐字标识符保持原色。上下文关键字（`value`、`when`、`where`）刻意缺省——无状态的字体内词法器分不清它们和普通名字。

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

唯一的非关键字语言：一个标记**标签状态机**通过字形替换走完 `标签名 → 属性名 → = → 值`。分隔符（`< > / =` 和引号）保持原色——它们是不着色的状态载体。`<3` 和带空格的 `< b` 保持原色（开标签需要标识符前瞻），实体只在文本中着色，未闭合的标签在行末终止，和这里的每个状态一样。

```html
<div class="hero">
  <!-- tag machine: names blue, attrs light, values orange -->
  <a href="https://example.io/x?a=1&amp;b=2">a link</a> &amp; plain text
  <img src='logo.png' alt="logo" />
</div>
```

## CSS

选择器、属性和值：属性名只在后面跟着 `:` 时才着色（约 365 个标准属性名），所以 `a:hover` 仍是选择器；值不着色直到 `;`，其中的数字、字符串和注释正常着色。多行规则块依赖冒号前瞻——块状态不能跨行。

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
