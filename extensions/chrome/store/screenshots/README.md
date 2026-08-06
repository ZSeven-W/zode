# Chrome Web Store screenshots

Upload these PNGs in this order. They are all full-bleed `1280 × 800` images,
the preferred Chrome Web Store screenshot size.

1. `01-ask-current-page.png` — current-tab context and side-panel answers
2. `02-select-an-element.png` — element picker and selected-element chip
3. `03-track-browser-tasks.png` — visible browser-task timeline
4. `04-approve-changes.png` — inline approval for an important action
5. `05-use-your-real-chrome.png` — paired Chrome session and localhost bridge

The neutral browser pages contain fictional, non-sensitive content. The Zode
features depicted correspond to the current extension: page-context prompts,
element selection, browser task activity, approvals, and the local bridge.
Refresh the screenshots from `source/marketing.html` before upload whenever
the side-panel UI or advertised behavior changes.

To render a single image locally:

```sh
'/Applications/Google Chrome.app/Contents/MacOS/Google Chrome' \
  --headless=new --hide-scrollbars --allow-file-access-from-files \
  --window-size=1280,800 \
  --screenshot=01-ask-current-page.png \
  'file:///absolute/path/to/extensions/chrome/store/screenshots/source/marketing.html?shot=1'
```
