# openmanuals

Workshop manuals by VIN or by model, read comfortably on a phone or a PC.

The factory tools these manuals come from are hard to use today: GlobalTIS
runs in Internet Explorer on Windows XP. The content is good. openmanuals is
one small server holding the manuals and a web app that asks it for one
page at a time. Nothing is downloaded whole to the phone.

```
phone / PC browser  ──HTTP──>  openmanuals (PC, NAS)  ──>  packs/*.sqlite
```

## What it does

- **VIN.** Decodes it with each manual's own VIN tables: make, model, year
  and, where the VIN says, engine.
- **Model.** Without a VIN, narrows the vehicle down step by step: make,
  model, year, engine, transmission. Engine and transmission can be left
  open.
- **Contents tree.** Shows the manual's own contents for that vehicle.
  Branches that do not apply to it are left out.
- **Pages that change per vehicle.** Where a page gives different values per
  engine or year, only the vehicle's own value is shown.
- **Search.** Full text, in the reader's language, limited to what applies to
  the vehicle.
- **Drawings.** Wiring diagrams and exploded views are vector drawings (SVG)
  and zoom without limit. Photographs open in a zoomable viewer.
- **Install and offline.** Installable as an app (PWA). Pages already opened
  still open without coverage.
- **Languages.** Spanish, English, German and Portuguese, as the source
  carries them.

## Running

```
cargo build --release
target/release/openmanuals --packs <folder with *.sqlite> [--listen 0.0.0.0:8460]
```

Then open `http://<host>:8460`. To reach it from outside the house, put it
behind something that already reaches home, such as a VPN or a tunnel. It
has no accounts of its own.

## Packs

A pack is one SQLite file made from a factory database by a tool in the
private `openmanuals-plugins` repository. The server recognises the kind
from `meta.source`:

| kind | content |
|---|---|
| `globaltis` | GM Europe service information: Opel, Vauxhall, Chevrolet Europe, Holden, Cadillac Europe |

Each kind has an adapter in `src/` behind the `Pack` trait (`src/pack.rs`).
The web app only sees the HTTP API below.

## API

| path | answer |
|---|---|
| `/api/sources` | the packs loaded |
| `/api/vin/{vin}` | vehicles every pack reads from that VIN |
| `/api/s/{src}/options?ctx=` | next choices for a vehicle known this far |
| `/api/s/{src}/describe?ctx=` | names of what `ctx` pins down |
| `/api/s/{src}/roots?ctx=` | trees offered for the vehicle |
| `/api/s/{src}/node/{id}?ctx=` | a tree node's branches and pages that apply |
| `/api/s/{src}/search?q=&ctx=` | pages matching the words |
| `/api/s/{src}/doc/{id}` | a page, as HTML |
| `/api/s/{src}/img/{id}` | a picture or drawing |

All of them take `lang` (`es`, `en`, `de`, `pt`). `ctx` is the vehicle as
`domain:value` pairs, for example `2:1,6:36,3:2011,7:254`.

## Licence

MIT, see [`LICENSE`](LICENSE). The manuals are not part of this repository.
