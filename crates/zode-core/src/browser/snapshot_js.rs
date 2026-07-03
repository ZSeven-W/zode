//! Injected snapshot script: tags visible interactive/text elements
//! with data-zode-ref="N" and returns an indented text outline
//! `[N] <tag> role text` — refs are stable until the next snapshot.

pub(crate) const SNAPSHOT_JS: &str = r#"
(() => {
  document.querySelectorAll('[data-zode-ref]')
    .forEach(el => el.removeAttribute('data-zode-ref'));
  let n = 0;
  const lines = [];
  const visible = el => {
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  const walk = (el, depth) => {
    if (!(el instanceof Element) || !visible(el)) return;
    const tag = el.tagName.toLowerCase();
    const interactive = ['a','button','input','textarea','select','label'].includes(tag)
      || el.onclick != null || el.getAttribute('role') === 'button';
    const ownText = Array.from(el.childNodes)
      .filter(c => c.nodeType === 3)
      .map(c => c.textContent.trim())
      .join(' ')
      .slice(0, 80);
    if (interactive || ownText) {
      n += 1;
      el.setAttribute('data-zode-ref', String(n));
      const extra = tag === 'input' ? ` type=${el.type} value=${JSON.stringify(el.value).slice(0,40)}` : '';
      lines.push(`${'  '.repeat(depth)}[${n}] <${tag}>${extra} ${ownText}`);
    }
    for (const c of el.children) walk(c, depth + 1);
  };
  walk(document.body, 0);
  return { count: n, outline: lines.join('\n') };
})()
"#;

#[cfg(test)]
mod tests {
    #[test]
    fn snapshot_js_is_self_contained_iife() {
        let js = super::SNAPSHOT_JS.trim();
        assert!(
            js.starts_with("(()"),
            "must be an IIFE for Runtime.evaluate"
        );
        assert!(js.ends_with(")()"), "must self-invoke");
        assert!(js.contains("data-zode-ref"));
    }
}
