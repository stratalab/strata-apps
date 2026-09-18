# strata-apps

Applications built on [StrataDB](https://stratadb.org), each one a working
program rather than a snippet. They exist to show what the database does under
real use, and to find out where it does not.

| app | what it is |
| --- | --- |
| [`strata-colonies`](strata-colonies) | Conway's Game of Life where tapping one cell forks the database. Runs the real engine as WebAssembly, live at [stratadb.org/demos/colonies](https://stratadb.org/demos/colonies/). |

More will land here. One folder per app, self-contained.

## Colonies and the website

`strata-colonies/web` is the source for the demo published on stratadb.org. The
website copies it in and hash-guards the copy, so editing it there fails the
website build. Changes go here and come back through:

```sh
cd strata-colonies
npm run website:sync -- /path/to/stratadb.org
```

CI runs the suite against whatever strata-core release is current, daily,
because an engine release can break a demo without a commit landing in either
repository. That has happened once.

## Building

Each app takes `stratadb` from the published strata-core tag, so a checkout
builds without anything else present:

```sh
cd strata-colonies
cargo check
```

To work against a local strata-core checkout, add a `[patch]` in an untracked
`.cargo/config.toml` rather than editing the dependency.

Generated databases are ignored by shape (`*-db/`, `*-db.*/`) because they
reach tens of gigabytes and stress runs are named per run.
