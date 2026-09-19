# Slack share assets

| Asset | Format | Use |
|---|---|---|
| [Results card](results-card.png) | PNG · 1200×675 | Recommended for the main post; designed editorial graphic, not a screenshot |
| [Walkthrough](walkthrough.gif) | GIF · 1200×800 · 12 seconds | Three scenes: deletion experiment, execution boundary, recorded call |
| [Walkthrough video](walkthrough.mp4) | H.264 MP4 · 1200×800 · 12 seconds · no audio | Smaller video version of the same walkthrough |
| [Video poster](walkthrough-poster.png) | PNG · 1200×800 | First scene of the walkthrough |
| [Technical evidence](../screenshots/historical-call.png) | PNG · 1072×331 | Real recorded historical-call waterfall |

The walkthrough uses edited captures of the public site, not a live devnet screencast.
The results card is an AI-generated editorial graphic checked against the measured counts.
The 5,907-line reduction is a separate source experiment: 2,743 production Rust lines plus
3,164 test/benchmark lines, including comments and blanks. It is not deployed in the runnable
demo; full historical derivation/proof routing still needs integration. See the
[deletion report](../../docs/retirement.md) and [call measurement](../../docs/history-performance.md#recorded-call-waterfall).

## Suggested post

> Can we stop accumulating old fork implementations in the active Base client?
>
> Base Era explores pinned, version-isolated historical workers. The local devnet runs across a
> real fork boundary; a separate latest-only deletion experiment removes **~5.9k net Rust lines:
> 2.7k production + 3.2k tests**.
>
> The goal is repeatable fork retirement while keeping history verifiable. Still a spike—full
> historical derivation/proof routing remains work.
>
> Demo, diff and measurements: https://refcell.github.io/base-era/
