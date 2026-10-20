# 工作原理

```
tokenize+theme → compile → fontbuild → verify
```

1. **tokenize + theme**：syntect 用真实的 TextMate 语法解析代码，再按每套主题把每个字符解析成具体颜色。
2. **compile**（slot 模型）：一个 slot 是去重后的"每主题一色"颜色向量。每段着色文本变成一条链式上下文替换规则，上下文取能唯一锚定这次出现的最小窗口，并在全部代码块范围内审计误匹配。
3. **fontbuild**：源字体的表尽可能逐字节保留；着色字形是零轮廓字形加 COLRv0 图层引用到基础字形，每套主题一个 CPAL 调色板。
4. **verify**：harfrust 在进程内对产出字体做 shaping，逐字符比对替换后的字形 id，再从 CPAL 读回每个调色板校验。

## 关键取舍

- 没有字形的字符（换行、emoji）是**硬屏障**：浏览器的字体回退会把它们踢出本字体的 shaping run，规则上下文永不跨越它们。
- 规则只依赖 slot，所以**一份 GSUB 服务所有主题**——切换调色板零重排。
- 长 token（注释、字符串）在 shaper 64 字形的上下文预算内切成链式片段。

下面的 Python 块演示多行结构——每个换行都是屏障，字符串着色在行内闭合：

```python
MAX_RETRIES = 3  # UPPER_CASE assignment = constant (semantic layer)

def fetch(url, *, retries=MAX_RETRIES):
    """GET with retries. A triple-quoted docstring never closes a ' quote."""
    for attempt in range(retries):
        try:
            return request("GET", url)
        except TimeoutError as exc:
            log.warning(f"attempt {attempt} failed: {exc}")
    raise ConnectionError(f"no answer after {retries} retries")
```

## 预言机

每个字体出厂前都要通过预言机：一个参考词法器，加上数百个由语言规范派生的确定性随机样本，与 shaper 的行为逐字形对齐。

```ts
// Semantic layer: definitions, call sites, parameters, properties and
// built-in types each get their own color slot
const registry = new Map<string, () => Promise<Response>>()

async function retry(fn: () => Promise<Response>, times: number) {
  for (let i = 0; i < times; i++) {
    try {
      return await fn()
    } catch (err) {
      if (i === times - 1) throw err
    }
  }
  throw new Error('unreachable')
}
```
