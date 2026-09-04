//! The report's fonts and stylesheet (spec eval-scorecard-report R6).
//!
//! Both are embedded at build time. A report is read months after the run
//! directories it describes were deleted, often from a laptop with no network
//! and no access to the machine that produced it, so a `<link>` to anywhere is
//! a broken report waiting to happen.

use base64::engine::general_purpose::STANDARD;
use base64::Engine;

/// IBM Plex, latin subset, under the SIL Open Font License 1.1. The licence
/// text sits beside the files and is reproduced in every report (R6).
const SANS_400: &[u8] = include_bytes!("../../../../../assets/fonts/IBMPlexSans-Regular.woff2");
const SANS_600: &[u8] = include_bytes!("../../../../../assets/fonts/IBMPlexSans-SemiBold.woff2");
const MONO_400: &[u8] = include_bytes!("../../../../../assets/fonts/IBMPlexMono-Regular.woff2");
pub const OFL: &str = include_str!("../../../../../assets/fonts/OFL.txt");

fn face(family: &str, weight: u16, bytes: &[u8]) -> String {
    format!(
        "@font-face{{font-family:'{family}';font-style:normal;font-weight:{weight};\
         font-display:swap;src:url(data:font/woff2;base64,{}) format('woff2')}}",
        STANDARD.encode(bytes)
    )
}

/// The `<style>` element's contents: three faces, then the sheet.
pub fn stylesheet() -> String {
    let mut css = String::with_capacity(160 * 1024);
    css.push_str(&face("IBM Plex Sans", 400, SANS_400));
    css.push_str(&face("IBM Plex Sans", 600, SANS_600));
    css.push_str(&face("IBM Plex Mono", 400, MONO_400));
    css.push_str(SHEET);
    css
}

/// Tokens are declared once on bare `:root` so the page renders correctly in
/// the un-stamped default state, and redefined for dark. Every colour below
/// comes from a token; none is defined only inside the media query.
const SHEET: &str = r#"
:root{
  --bg:#f7f7f6; --panel:#ffffff; --ink:#1a1c1e; --ink-soft:#5b6167;
  --rule:#dcdfe2; --rule-soft:#eceef0;
  --accent:#1c5f8b; --accent-soft:#e6eff5;
  --pass:#1a6b47; --pass-bg:#e4f2ea;
  --fail:#a3282a; --fail-bg:#fbe8e8;
  --warn:#8a5a10; --warn-bg:#fbf0dc;
  --s0:#1c5f8b; --s1:#8a4f9e; --s2:#1a6b47; --s3:#a35c17;
  --mono:'IBM Plex Mono',ui-monospace,SFMono-Regular,Menlo,monospace;
  --sans:'IBM Plex Sans',system-ui,-apple-system,Segoe UI,Roboto,sans-serif;
}
@media (prefers-color-scheme:dark){
  :root:not([data-theme="light"]){
    --bg:#131517; --panel:#1b1e21; --ink:#e8eaec; --ink-soft:#a0a7ae;
    --rule:#2e3338; --rule-soft:#24282c;
    --accent:#69b3e0; --accent-soft:#17303f;
    --pass:#6fcf9a; --pass-bg:#16301f;
    --fail:#f08b8b; --fail-bg:#361a1a;
    --warn:#e2b06c; --warn-bg:#33270f;
    --s0:#69b3e0; --s1:#c191d6; --s2:#6fcf9a; --s3:#e0a061;
  }
}
:root[data-theme="dark"]{
  --bg:#131517; --panel:#1b1e21; --ink:#e8eaec; --ink-soft:#a0a7ae;
  --rule:#2e3338; --rule-soft:#24282c;
  --accent:#69b3e0; --accent-soft:#17303f;
  --pass:#6fcf9a; --pass-bg:#16301f;
  --fail:#f08b8b; --fail-bg:#361a1a;
  --warn:#e2b06c; --warn-bg:#33270f;
  --s0:#69b3e0; --s1:#c191d6; --s2:#6fcf9a; --s3:#e0a061;
}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--ink);font-family:var(--sans);
  font-size:14px;line-height:1.55;-webkit-text-size-adjust:100%}
main{max-width:78rem;margin:0 auto;padding:2.5rem 1.5rem 5rem;
  display:flex;flex-direction:column;gap:2.25rem}
h1{font-size:1.5rem;font-weight:600;margin:0;letter-spacing:-.01em;text-wrap:balance}
h2{font-size:.75rem;font-weight:600;margin:0 0 .85rem;text-transform:uppercase;
  letter-spacing:.09em;color:var(--ink-soft)}
h3{font-size:.95rem;font-weight:600;margin:0 0 .5rem}
p{margin:0 0 .6rem}
section{background:var(--panel);border:1px solid var(--rule);border-radius:6px;padding:1.35rem 1.5rem}
header.report{display:flex;flex-direction:column;gap:.35rem;padding:0;background:none;border:none}
header.report .sub{color:var(--ink-soft);font-size:.85rem}
.scroll{overflow-x:auto}
table{border-collapse:collapse;width:100%;font-size:.85rem}
th,td{text-align:left;padding:.42rem .7rem;border-bottom:1px solid var(--rule-soft);vertical-align:top}
th{font-weight:600;color:var(--ink-soft);font-size:.72rem;text-transform:uppercase;
  letter-spacing:.07em;white-space:nowrap;border-bottom:1px solid var(--rule)}
tbody tr:last-child td{border-bottom:none}
td.num,th.num{text-align:right;font-family:var(--mono);font-variant-numeric:tabular-nums;white-space:nowrap}
code,.mono{font-family:var(--mono);font-size:.9em}
dl.identity{display:grid;grid-template-columns:max-content 1fr;gap:.3rem 1.4rem;margin:0}
dl.identity dt{color:var(--ink-soft);font-size:.8rem}
dl.identity dd{margin:0;font-family:var(--mono);font-size:.82rem;overflow-wrap:anywhere}
.pill{display:inline-block;padding:.05rem .45rem;border-radius:3px;font-size:.72rem;
  font-weight:600;letter-spacing:.04em;white-space:nowrap}
.pill.pass{color:var(--pass);background:var(--pass-bg)}
.pill.fail{color:var(--fail);background:var(--fail-bg)}
.pill.warn{color:var(--warn);background:var(--warn-bg)}
.pill.mute{color:var(--ink-soft);background:var(--rule-soft)}
ul.notes{margin:0;padding-left:1.1rem}
ul.notes li{margin-bottom:.25rem}
ul.notes li:last-child{margin-bottom:0}
.empty{color:var(--ink-soft);font-style:italic}
details{margin-top:.2rem}
details>summary{cursor:pointer;color:var(--accent);font-size:.8rem}
.reasoning{font-size:.82rem;color:var(--ink-soft);max-width:64ch}
a{color:var(--accent)}
.meta{color:var(--ink-soft);font-size:.8rem}
.mute-text{color:var(--ink-soft)}
h3.run{font-family:var(--mono);font-size:.8rem;font-weight:400;color:var(--ink-soft);
  margin:1.4rem 0 .4rem;overflow-wrap:anywhere}
section>h3.run:first-of-type{margin-top:0}
.legend{display:flex;flex-wrap:wrap;gap:.9rem;margin:0 0 .8rem}
.key{font-size:.78rem;display:inline-flex;align-items:center;gap:.35rem}
.key::before{content:"";width:.7rem;height:.7rem;border-radius:50%;background:currentColor}
.key.s0{color:var(--s0)} .key.s1{color:var(--s1)}
.key.s2{color:var(--s2)} .key.s3{color:var(--s3)}
.charts{display:grid;grid-template-columns:repeat(auto-fit,minmax(20rem,1fr));gap:1.2rem}
figure.chart{margin:0;min-width:0}
figure.chart svg{width:100%;height:auto;display:block}
figure.chart figcaption{font-size:.8rem;font-weight:600;margin-bottom:.2rem}
.chart text{font-family:var(--sans);font-size:10px;fill:var(--ink-soft)}
.chart .gate-label{fill:var(--fail)}
.chart .flip-note{fill:var(--warn)}
.chart .gate{stroke:var(--fail);stroke-dasharray:4 3;stroke-width:1}
.chart .axis{stroke:var(--rule);stroke-width:1}
.chart .series{fill:none;stroke-width:1.6}
.chart .series.s0{stroke:var(--s0)} .chart .series.s1{stroke:var(--s1)}
.chart .series.s2{stroke:var(--s2)} .chart .series.s3{stroke:var(--s3)}
.chart .pt.s0{fill:var(--s0)} .chart .pt.s1{fill:var(--s1)}
.chart .pt.s2{fill:var(--s2)} .chart .pt.s3{fill:var(--s3)}
.chart .pt.flip{fill:var(--panel);stroke:var(--warn);stroke-width:2.5}
footer{color:var(--ink-soft);font-size:.75rem;border-top:1px solid var(--rule);padding-top:1rem}
footer pre{white-space:pre-wrap;font-family:var(--mono);font-size:.68rem;margin:.4rem 0 0}
"#;
