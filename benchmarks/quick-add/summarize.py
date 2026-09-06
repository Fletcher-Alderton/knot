#!/usr/bin/env python3
"""Summarize measured Quick Add JSONL runs; never substitutes missing results."""
import argparse
from collections import defaultdict
import json
import math
from pathlib import Path
import statistics


def percentile(values, fraction):
    ordered = sorted(values)
    return ordered[max(0, math.ceil(len(ordered) * fraction) - 1)]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("inputs", nargs="+", type=Path)
    parser.add_argument("--output-dir", type=Path, required=True)
    args = parser.parse_args()
    groups = defaultdict(list)
    seen = set()
    for source in args.inputs:
        for number, line in enumerate(source.read_text().splitlines(), 1):
            if not line.strip():
                continue
            row = json.loads(line)
            key = (row["model"], row["mode"], row["id"])
            if key in seen:
                raise ValueError(f"Duplicate result {key} at {source}:{number}")
            seen.add(key)
            groups[key[:2]].append(row)
    fields = ("title", "body", "column", "labels", "due", "start")
    summaries = []
    failures = []
    for (model, mode), rows in sorted(groups.items()):
        latency = [row["latency_ms"] / 1000 for row in rows]
        valid = sum(row["schema_valid"] for row in rows)
        correct = sum(row["strict_core_success"] for row in rows)
        per_field = {field: sum(row.get("field_accuracy", {}).get(field) is True
                                and row["schema_valid"] for row in rows)
                     for field in fields}
        summaries.append({"model": model, "mode": mode, "n": len(rows),
                          "schema_valid": valid, "all_fields_correct": correct,
                          "field_correct": per_field,
                          "latency_seconds": {"median": statistics.median(latency),
                                              "p95": percentile(latency, .95),
                                              "mean": statistics.mean(latency),
                                              "min": min(latency), "max": max(latency)}})
        for row in rows:
            if not row["strict_core_success"]:
                failures.append({"model": model, "mode": mode, "id": row["id"],
                                 "input": row["input"], "error": row.get("error"),
                                 "incorrect_fields": [field for field in fields
                                                      if row.get("field_accuracy", {}).get(field) is not True],
                                 "raw_output": row.get("raw_output")})
    args.output_dir.mkdir(parents=True, exist_ok=True)
    (args.output_dir / "summary.json").write_text(json.dumps(summaries, indent=2) + "\n")
    (args.output_dir / "failures.json").write_text(json.dumps(failures, indent=2, ensure_ascii=False) + "\n")
    lines = ["# Quick Add measured results", "",
             "All-fields accuracy requires title, body, column, labels, due and start to match the predefined expectations. Errors count as failures. Confidence and warning wording are not accuracy targets.", "",
             "| Model | Mode | Inputs | Valid schema | All fields correct | Median | p95 |",
             "|---|---|---:|---:|---:|---:|---:|"]
    for row in summaries:
        n = row["n"]
        times = row["latency_seconds"]
        lines.append(f"| {row['model']} | {row['mode']} | {n} | {row['schema_valid']}/{n} | {row['all_fields_correct']}/{n} ({row['all_fields_correct']/n:.0%}) | {times['median']:.2f}s | {times['p95']:.2f}s |")
    lines += ["", "## Field accuracy", "",
              "| Model | Mode | Title | Body | Column | Labels | Due | Start |",
              "|---|---|---:|---:|---:|---:|---:|---:|"]
    for row in summaries:
        cells = " | ".join(f"{row['field_correct'][field]}/{row['n']}" for field in fields)
        lines.append(f"| {row['model']} | {row['mode']} | {cells} |")
    lines += ["", "Warm runs preload the model weights before timing; cold runs unload them before each timed request. The OS filesystem cache is not purged. Both paths create a fresh inference context per request. Timings describe this machine and runtime, not other hardware or providers.", "",
              "See failures.json for every unsuccessful input and the JSONL source files for raw outputs and individual timings.", ""]
    (args.output_dir / "summary.md").write_text("\n".join(lines))
    print("\n".join(lines))


if __name__ == "__main__":
    main()
