# Blitz Website

A website for [Blitz](https://github.com/dioxuslabs/blitz)

## Updating data

The CSS property popularity stats in `data/css-popularity.json` are sourced from
[Chrome Status](https://chromestatus.com/data/csspopularity). To refresh them,
run the update script (requires `curl` and `jq`):

```sh
bash scripts/update-css-popularity.sh
```