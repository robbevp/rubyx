"""Data transformation — sync generators for streaming."""

import json
import csv
import io


def csv_to_records(csv_text):
    """Parse CSV string into list of dicts. Sync, returns all at once."""
    reader = csv.DictReader(io.StringIO(csv_text))
    return [row for row in reader]


def records_to_csv(records):
    """Convert list of dicts to CSV string."""
    if not records:
        return ""
    output = io.StringIO()
    writer = csv.DictWriter(output, fieldnames=records[0].keys())
    writer.writeheader()
    writer.writerows(records)
    return output.getvalue()


def stream_json_lines(json_text):
    """Stream-parse newline-delimited JSON. Yields one record at a time."""
    for line in json_text.strip().split('\n'):
        line = line.strip()
        if line:
            yield json.loads(line)


def stream_transform(records, transform_fn):
    """Apply a transform to each record, yielding results one by one.

    Usage from Ruby:
        gen = data_transform.stream_transform(records, lambda r: {**r, 'processed': True})
        Rubyx.stream(gen).each { |r| puts r.to_ruby }
    """
    for record in records:
        yield transform_fn(record)


def stream_filter(records, predicate):
    """Filter records using a predicate, streaming results.

    Usage from Ruby:
        gen = data_transform.stream_filter(records, lambda r: int(r['age']) > 21)
        Rubyx.stream(gen).each { |r| puts r.to_ruby }
    """
    for record in records:
        if predicate(record):
            yield record


def batch_process(items, batch_size=10):
    """Yield items in batches. Useful for bulk API calls."""
    batch = []
    for item in items:
        batch.append(item)
        if len(batch) >= batch_size:
            yield batch
            batch = []
    if batch:
        yield batch
