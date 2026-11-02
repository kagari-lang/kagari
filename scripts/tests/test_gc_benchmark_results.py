"""Finite fixture contracts and process/pair estimators for GC evidence."""
import copy
import csv
import io
from pathlib import Path

import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from gc_benchmark_results import EvidenceError, SUITES, parse_run, summarize


HEADERS = {
    "access": "process,phase,sample,case,iterations,elapsed_ns,checksum",
    "lifecycle": "process,phase,sample,engine,case,count,elapsed_ns",
    "references": "sample,kind,phase,count,elapsed_ns",
    "collection": "record,sample,case,count,stage,elapsed_ns,work,steps,maintenance_ns",
    "sustained": (
        "sample,case,batch,allocations,allocation_ns,total_ns,service,service_ns,work,"
        "retired,phase,before_objects,objects,peak_managed_bytes,cycles,maintenance_ns,"
        "backing_bytes,indirect_bytes,allocation_growth_bytes"
    ),
    "capacity": (
        "case,stage,backing_bytes,pages,page_slots,page_table_bytes,roots,root_slots,"
        "root_table_bytes,descriptors,descriptor_slots,descriptor_table_bytes,"
        "mark_queue_bytes,release_queue_bytes,active_bytes,collect_ns,maintenance_ns"
    ),
}
ACCESS = (
    "plain_read", "plain_write", "plain_increment", "typed_read", "typed_write",
    "typed_increment", "dynamic_read", "dynamic_write", "dynamic_increment",
    "typed_borrow_each_increment", "typed_borrow_batch_increment",
)
METRIC_KEYS = {"case", "metric", "unit", "direction", "value", "observations"}
SUMMARY_KEYS = {
    "suite", "case", "metric", "unit", "direction", "variant", "median", "min", "max",
    "processes", "observations", "process_values",
}


def family(suite):
    if suite.startswith("access"):
        return "access"
    if suite.startswith("lifecycle"):
        return "lifecycle"
    return "sustained" if suite == "burst" else suite


def encode(suite, rows):
    buffer = io.StringIO(newline="")
    writer = csv.DictWriter(buffer, HEADERS[family(suite)].split(","), lineterminator="\n")
    writer.writeheader()
    writer.writerows(rows)
    return buffer.getvalue()


def fixture(suite):
    rows = []
    if family(suite) == "access":
        cases = {"access": ACCESS,
                 "access-borrow": ("dynamic_borrow_each_increment", "dynamic_borrow_batch_increment"),
                 "access-alternating": ("typed_alternating_increment", "dynamic_alternating_increment")}[suite]
        for sample in range(14):
            for case in cases:
                checksum = 350000 if case.endswith("_read") else 49999 if case.endswith("_write") else 50000
                rows.append(dict(process=7, phase="warmup" if sample < 3 else "measure",
                                 sample=sample, case=case, iterations=50000,
                                 elapsed_ns=900000000 if sample < 3 else sample * 50000,
                                 checksum=checksum))
    elif family(suite) == "lifecycle":
        for sample in range(14):
            for engine in ("kagari", "gc_arena"):
                cases = [(case, 50000) for case in (
                    "scalar_read", "scalar_write", "scalar_increment", "borrow_each_increment",
                    "borrow_batch_increment", "root_copy_idle_32")]
                allocate = "allocate_prepared" if suite == "lifecycle-prepared" and engine == "kagari" else "allocate_rooted"
                cases += [(case, count) for count in (512, 8192)
                          for case in (allocate, "collect_live", "clear_roots", "collect_dead")]
                for case, count in cases:
                    rows.append(dict(process=7, phase="warmup" if sample < 3 else "sample",
                                     sample=sample, engine=engine, case=case, count=count,
                                     elapsed_ns=999999999 if sample < 3 else sample * count))
    elif suite == "references":
        for sample in range(14):
            for kind in ("typed", "dynamic"):
                for phase in ("Idle", "Mark", "Sweep"):
                    rows.append(dict(sample=sample, kind=kind, phase=phase, count=50000,
                                     elapsed_ns=999999999 if sample < 3 else sample * 50000))
    elif suite == "collection":
        for sample in range(14):
            for case in ("leaves", "duplicate", "chain"):
                for stage in ("roots", "mark", "sweep", "transition"):
                    times = list(range(1, 11)) if stage == "roots" else [20] if stage == "mark" else [0] if stage == "transition" else []
                    if sample < 3:
                        times = [time + 999999 for time in times]
                    for time in times:
                        rows.append(dict(record="step", sample=sample, case=case, count=8192,
                                         stage=stage, elapsed_ns=time, work=1, steps=1, maintenance_ns=0))
                    rows.append(dict(record="stage", sample=sample, case=case, count=8192,
                                     stage=stage, elapsed_ns=sum(times), work=len(times),
                                     steps=len(times), maintenance_ns=0))
                rows.append(dict(record="full", sample=sample, case=case, count=8192,
                                 stage="full", elapsed_ns=1000 if sample >= 3 else 9999999,
                                 work=123, steps=0, maintenance_ns=20))
    elif family(suite) == "sustained":
        for sample in range(14):
            for case in ("leaf", "cycle"):
                objects = 0
                for batch in range(129):
                    cleanup = batch == 128
                    allocations = 0 if cleanup else 4096 if suite == "burst" and batch % 16 == 0 else 256
                    service = "collect" if cleanup else "assist" if case == "cycle" and batch == 127 else "step" if batch >= 64 else "idle"
                    before, objects = objects, 0 if cleanup else objects + allocations
                    rows.append(dict(
                        sample=sample, case=case, batch=batch, allocations=allocations,
                        allocation_ns=0 if cleanup else (6000000 if sample < 3 else 60) + batch,
                        total_ns=1000 if cleanup else (10000000 if sample < 3 else 100) + batch,
                        service=service, service_ns={"idle": 0, "step": 10, "assist": 50, "collect": 800}[service],
                        work=0 if service == "idle" else 2000 if cleanup else 1024,
                        retired=0 if service == "idle" else 100 if cleanup else 32,
                        phase="Mark" if 64 <= batch < 127 else "Idle",
                        before_objects=before, objects=objects,
                        peak_managed_bytes=999999999 if sample < 3 else (before + allocations) * 12,
                        cycles=3 if cleanup else 2 if batch == 127 else 1 if batch >= 64 else 0,
                        maintenance_ns=0 if service == "idle" else 100 if cleanup else 2,
                        backing_bytes=objects * 8, indirect_bytes=objects * 4,
                        allocation_growth_bytes=allocations * 8,
                    ))
    else:
        for case in ("pages", "roots", "descriptors"):
            for stage in ("peak", "one_high_id", "empty"):
                row = dict.fromkeys(HEADERS["capacity"].split(",")[2:], 0)
                row.update(case=case, stage=stage, collect_ns=0 if stage == "peak" else 100,
                           maintenance_ns=0 if stage == "peak" else 20,
                           backing_bytes=8192 if stage == "peak" else 8 if stage == "one_high_id" else 0)
                rows.append(row)
    return rows


def metrics(suite, rows=None):
    return parse_run(suite, encode(suite, fixture(suite) if rows is None else rows), 7)


def lookup(rows, case, metric):
    return next(row for row in rows if row["case"] == case and row["metric"] == metric)


def process(index, value, variant="candidate", observations=11, suite="access"):
    return dict(suite=suite, variant=variant, process=index, metrics=[dict(
        case="plain_read", metric="ns_per_op", unit="ns/op", direction="lower",
        value=value, observations=observations)])


class CsvEvidenceTests(unittest.TestCase):
    def test_suite_order_and_launch_contract(self):
        self.assertEqual(list(SUITES.items()), [
            ("access", ("field_access", ())),
            ("access-borrow", ("field_access", ("--dynamic-borrow",))),
            ("access-alternating", ("field_access", ("--alternating",))),
            ("lifecycle", ("arena_comparison", ())),
            ("lifecycle-prepared", ("arena_comparison", ("--prepared",))),
            ("references", ("reference_access", ())),
            ("collection", ("collection_profile", ())),
            ("sustained", ("sustained_allocation", ())),
            ("burst", ("sustained_allocation", ("--burst",))),
            ("capacity", ("metadata_capacity", ())),
        ])

    def test_all_suites_have_exact_unique_metric_schema(self):
        for suite in SUITES:
            with self.subTest(suite=suite):
                result = metrics(suite)
                keys = [(m["case"], m["metric"]) for m in result]
                self.assertEqual(len(keys), len(set(keys)))
                for metric in result:
                    self.assertEqual(set(metric), METRIC_KEYS)
                    self.assertIn(metric["direction"], ("lower", "higher", "neutral"))
                # The public summarizer accepts every parser output, including None tails.
                summarize([dict(suite=suite, variant="candidate", process=7, metrics=result)])

    def test_each_suite_rejects_truncation_missing_case_and_duplicate(self):
        for suite in SUITES:
            original = fixture(suite)
            missing_case = original[0].get("case", original[0].get("kind"))
            for label, rows in (
                ("truncated", original[:-1]),
                ("missing case", [r for r in original if r.get("case", r.get("kind")) != missing_case]),
                ("duplicate", original + [original[-1]]),
                ("empty", []),
            ):
                with self.subTest(suite=suite, failure=label), self.assertRaises(EvidenceError):
                    metrics(suite, rows)

    def test_exact_headers_ragged_rows_noise_and_malformed_csv(self):
        for suite in SUITES:
            text = encode(suite, fixture(suite))
            header, body = text.split("\n", 1)
            for broken in (header + ",extra\n" + body, header.upper() + "\n" + body,
                           "noise\n" + text, text + "\n", text + "x\n", text + '"unterminated'):
                with self.subTest(suite=suite), self.assertRaises(EvidenceError):
                    parse_run(suite, broken, 7)

    def test_numeric_fields_are_unsigned_integers_even_in_warmup(self):
        for suite in SUITES:
            original = fixture(suite)
            for field, value in original[0].items():
                if not isinstance(value, int):
                    continue
                for invalid in (-1, "1.0", "nan", "inf", "", " 1", "+1", "1e3"):
                    rows = copy.deepcopy(original)
                    rows[0][field] = invalid
                    with self.subTest(suite=suite, field=field, invalid=invalid), self.assertRaises(EvidenceError):
                        metrics(suite, rows)

    def test_sample_ids_cannot_be_replaced_duplicated_or_extended(self):
        for suite in SUITES:
            if suite == "capacity":
                continue
            original = fixture(suite)
            for sample in (1, 14):
                rows = copy.deepcopy(original)
                rows[0]["sample"] = sample
                with self.subTest(suite=suite, sample=sample), self.assertRaises(EvidenceError):
                    metrics(suite, rows)

    def test_access_warmup_exclusion_checksums_and_counts(self):
        for suite, count in (("access", 11), ("access-borrow", 2), ("access-alternating", 2)):
            result = metrics(suite)
            self.assertEqual(len(result), count)
            self.assertTrue(all(m["value"] == 8 and m["observations"] == 11 for m in result))
        for field, value in (("checksum", 0), ("iterations", 49999), ("process", 0), ("phase", "measure")):
            rows = fixture("access")
            rows[0][field] = value
            with self.subTest(field=field), self.assertRaises(EvidenceError):
                metrics("access", rows)
        rows = fixture("access")
        for row in rows:
            row["elapsed_ns"] = 0
        self.assertTrue(all(m["value"] == 0 for m in metrics("access", rows)))

    def test_arena_case_count_engine_and_prepared_contract(self):
        for suite in ("lifecycle", "lifecycle-prepared"):
            result = metrics(suite)
            self.assertEqual(len(result), 28)
            self.assertTrue(all(m["value"] == 8 and m["observations"] == 11 for m in result))
            allocate = "allocate_prepared" if suite.endswith("prepared") else "allocate_rooted"
            self.assertEqual(lookup(result, f"kagari/{allocate}/8192", "ns_per_op")["value"], 8)
            self.assertEqual(lookup(result, "gc_arena/allocate_rooted/512", "ns_per_op")["value"], 8)
            for field, value in (("count", 32), ("engine", "other"), ("phase", "measure"), ("process", 2)):
                rows = fixture(suite)
                rows[-1][field] = value
                with self.subTest(suite=suite, field=field), self.assertRaises(EvidenceError):
                    metrics(suite, rows)
        with self.assertRaises(EvidenceError):
            parse_run("lifecycle-prepared", encode("lifecycle", fixture("lifecycle")), 7)

    def test_reference_phase_grid_and_count(self):
        result = metrics("references")
        self.assertEqual(len(result), 6)
        self.assertTrue(all(m["value"] == 8 and m["observations"] == 11 for m in result))
        for field, value in (("phase", "Roots"), ("count", 50001), ("kind", "plain")):
            rows = fixture("references")
            rows[0][field] = value
            with self.subTest(field=field), self.assertRaises(EvidenceError):
                metrics("references", rows)

    def test_invalid_public_arguments(self):
        for suite, text, index in (("unknown", "", 0), ("access", None, 0),
                                    ("access", "", -1), ("access", "", True)):
            with self.assertRaises(EvidenceError):
                parse_run(suite, text, index)


class CollectionTests(unittest.TestCase):
    def test_aggregates_do_not_double_count_steps_full_or_maintenance(self):
        result = metrics("collection")
        self.assertEqual(lookup(result, "leaves/roots", "elapsed_ns")["value"], 55)
        self.assertEqual(lookup(result, "leaves/roots", "work")["value"], 10)
        self.assertEqual(lookup(result, "leaves/full", "elapsed_ns")["value"], 1000)
        self.assertEqual(lookup(result, "leaves/full", "maintenance_ns")["value"], 20)
        self.assertEqual(lookup(result, "leaves/full", "work")["value"], 123)

    def test_nearest_rank_percentiles_and_absent_categories(self):
        result = metrics("collection")
        for suffix, value in (("p50", 5), ("p95", 10), ("p99", 10), ("max", 10)):
            metric = lookup(result, "leaves/roots", f"step_{suffix}_ns")
            self.assertEqual((metric["value"], metric["observations"]), (value, 110))
        metric = lookup(result, "leaves/sweep", "step_p99_ns")
        self.assertEqual((metric["value"], metric["observations"]), (None, 0))
        metric = lookup(result, "leaves/transition", "step_max_ns")
        self.assertEqual((metric["value"], metric["observations"]), (0, 11))

    def test_stage_totals_must_match_every_step_field(self):
        for field in ("elapsed_ns", "work", "steps", "maintenance_ns"):
            rows = fixture("collection")
            row = next(r for r in rows if r["record"] == "stage")
            row[field] += 1
            with self.subTest(field=field), self.assertRaises(EvidenceError):
                metrics("collection", rows)
        rows = fixture("collection")
        del rows[0]
        with self.assertRaises(EvidenceError):
            metrics("collection", rows)
        rows = fixture("collection")
        rows.append(copy.deepcopy(rows[0]))
        with self.assertRaises(EvidenceError):
            metrics("collection", rows)

    def test_normal_step_shape_and_workload_count(self):
        for field, value in (("count", 8193), ("steps", 2), ("work", 1025),
                             ("maintenance_ns", 1), ("stage", "full")):
            rows = fixture("collection")
            rows[0][field] = value
            with self.subTest(field=field), self.assertRaises(EvidenceError):
                metrics("collection", rows)

    def test_full_is_separate_but_requires_step_work(self):
        for field, value in (("steps", 1), ("work", 0), ("maintenance_ns", 1001), ("stage", "mark")):
            rows = fixture("collection")
            row = next(r for r in rows if r["record"] == "full" and r["sample"] == 3)
            row[field] = value
            with self.subTest(field=field), self.assertRaises(EvidenceError):
                metrics("collection", rows)
        rows = fixture("collection")
        rows = [r for r in rows if r["record"] != "step"]
        for row in rows:
            if row["record"] == "stage":
                row.update(elapsed_ns=0, work=0, steps=0)
        with self.assertRaises(EvidenceError):
            metrics("collection", rows)


class SustainedTests(unittest.TestCase):
    def test_weighted_costs_tails_counters_and_cleanup_scope(self):
        result = metrics("sustained")
        self.assertEqual(lookup(result, "leaf", "ns_per_object")["value"], 163.5 / 256)
        self.assertEqual(lookup(result, "leaf", "allocation_ns_per_object")["value"], 123.5 / 256)
        self.assertAlmostEqual(lookup(result, "leaf", "ops_per_second")["value"], 256e9 / 163.5)
        self.assertAlmostEqual(lookup(result, "leaf", "gc_service_fraction")["value"], 640 / (128 * 163.5))
        self.assertEqual(lookup(result, "leaf", "lifecycle_ns_per_object")["value"], (128 * 163.5 + 1000) / 32768)
        self.assertEqual(lookup(result, "leaf", "cycles")["value"], 2)
        self.assertEqual(lookup(result, "leaf", "maintenance_ns")["value"], 128)
        self.assertEqual(lookup(result, "leaf", "cleanup_ns")["value"], 1000)
        self.assertEqual(lookup(result, "leaf", "cleanup_work")["value"], 2000)
        for suffix, value in (("p50", 163), ("p95", 221), ("p99", 226), ("max", 227)):
            m = lookup(result, "leaf", f"batch_{suffix}_ns")
            self.assertEqual((m["value"], m["observations"]), (value, 1408))
        self.assertEqual(lookup(result, "leaf", "step_count")["value"], 64)
        self.assertEqual(lookup(result, "leaf", "step_total_ns")["value"], 640)
        self.assertEqual(lookup(result, "cycle", "assist_p99_ns")["observations"], 11)
        self.assertEqual(lookup(result, "leaf", "assist_p99_ns")["value"], None)
        self.assertEqual(lookup(result, "leaf", "assist_max_ns")["observations"], 0)

    def test_weighting_is_not_a_median_of_sample_costs(self):
        rows = fixture("sustained")
        for row in rows:
            if row["sample"] == 3 and row["batch"] < 128:
                row["total_ns"] *= 11
        result = metrics("sustained", rows)
        self.assertAlmostEqual(lookup(result, "leaf", "ns_per_object")["value"], 163.5 * 21 / 11 / 256)
        self.assertEqual(lookup(result, "leaf", "step_count")["value"], 64)

    def test_event_tails_pool_but_run_counters_and_totals_use_medians(self):
        rows = fixture("sustained")
        for row in rows:
            if row["sample"] == 3 and row["case"] == "leaf" and row["service"] == "step":
                row["service"] = "assist"
        result = metrics("sustained", rows)
        self.assertEqual(lookup(result, "leaf", "step_count")["value"], 64)
        self.assertEqual(lookup(result, "leaf", "step_total_ns")["value"], 640)
        self.assertEqual(lookup(result, "leaf", "assist_count")["value"], 0)
        self.assertEqual(lookup(result, "leaf", "assist_total_ns")["value"], 0)
        tail = lookup(result, "leaf", "assist_p99_ns")
        self.assertEqual((tail["value"], tail["observations"]), (10, 64))

    def test_assists_are_not_subject_to_normal_step_budget(self):
        rows = fixture("sustained")
        for row in rows:
            if row["service"] == "assist":
                row.update(work=8192, retired=128)
        self.assertEqual(lookup(metrics("sustained", rows), "cycle", "assist_count")["value"], 1)

    def test_active_window_cleanup_finishes_two_cycles(self):
        rows = fixture("sustained")
        for row in rows:
            if row["batch"] == 127:
                row["phase"] = "Mark"
            if row["batch"] == 128:
                row["cycles"] = 4
        self.assertEqual(lookup(metrics("sustained", rows), "leaf", "cycles")["value"], 2)

    def test_memory_counters_peaks_and_quarter_trend(self):
        result = metrics("sustained")
        self.assertEqual(lookup(result, "leaf", "peak_managed_bytes")["value"], 32768 * 12)
        self.assertEqual(lookup(result, "leaf", "observed_peak_objects")["value"], 32768)
        self.assertEqual(lookup(result, "leaf", "end_objects")["value"], 32768)
        self.assertEqual(lookup(result, "leaf", "end_backing_bytes")["value"], 32768 * 8)
        self.assertEqual(lookup(result, "leaf", "managed_bytes_q1")["value"], 16.5 * 256 * 12)
        self.assertEqual(lookup(result, "leaf", "managed_bytes_q4")["value"], 112.5 * 256 * 12)
        self.assertFalse(any("rss" in m["metric"].lower() for m in result))

    def test_burst_schedule_and_denominator(self):
        result = metrics("burst")
        allocations = 8 * 4096 + 120 * 256
        self.assertEqual(lookup(result, "leaf", "ns_per_object")["value"], 128 * 163.5 / allocations)
        for suite, wrong_suite in (("burst", "sustained"), ("sustained", "burst")):
            with self.subTest(suite=suite), self.assertRaises(EvidenceError):
                parse_run(suite, encode(wrong_suite, fixture(wrong_suite)), 7)

    def test_cleanup_timing_service_and_budget_invariants(self):
        mutations = (
            (0, "allocations", 255), (0, "service", "step_unknown"),
            (0, "service_ns", 1), (0, "work", 1), (0, "maintenance_ns", 1),
            (0, "retired", 1), (0, "allocation_ns", 999999999),
            (64, "work", 1025), (64, "retired", 33),
            (64, "service_ns", 999999999), (64, "maintenance_ns", 11),
            (65, "cycles", 0), (65, "before_objects", 0), (65, "phase", "Unknown"),
            (128, "objects", 1), (128, "backing_bytes", 1), (128, "indirect_bytes", 1),
            (128, "allocations", 256), (128, "allocation_ns", 1),
            (128, "service", "assist"), (128, "phase", "Mark"),
            (128, "cycles", 2), (128, "work", 0), (128, "retired", 0),
        )
        original = fixture("sustained")
        for batch, field, value in mutations:
            rows = copy.deepcopy(original)
            row = next(r for r in rows if r["sample"] == 3 and r["case"] == "leaf" and r["batch"] == batch)
            row[field] = value
            with self.subTest(batch=batch, field=field), self.assertRaises(EvidenceError):
                metrics("sustained", rows)

    def test_zero_clock_resolution_is_not_missing_latency_or_infinity(self):
        rows = fixture("sustained")
        for row in rows:
            row.update(allocation_ns=0, total_ns=0, service_ns=0, maintenance_ns=0)
        result = metrics("sustained", rows)
        self.assertEqual(lookup(result, "leaf", "batch_p99_ns")["value"], 0)
        self.assertEqual(lookup(result, "leaf", "step_p99_ns")["observations"], 704)
        self.assertEqual(lookup(result, "leaf", "ops_per_second")["value"], None)
        self.assertEqual(lookup(result, "leaf", "gc_service_fraction")["observations"], 0)


class CapacityAndSummaryTests(unittest.TestCase):
    def test_capacity_has_one_observation_and_actual_requested_counter_names(self):
        result = metrics("capacity")
        self.assertEqual(len(result), 9 * 15)
        self.assertTrue(all(m["observations"] == 1 for m in result))
        self.assertEqual(lookup(result, "pages/peak", "backing_bytes")["value"], 8192)
        self.assertEqual(lookup(result, "pages/empty", "maintenance_ns")["value"], 20)
        self.assertFalse(any("rss" in m["metric"].lower() for m in result))
        for field, value in (("collect_ns", 1), ("pages", 1)):
            rows = fixture("capacity")
            rows[0][field] = value
            with self.subTest(field=field), self.assertRaises(EvidenceError):
                metrics("capacity", rows)

    def test_medians_are_of_process_estimates_not_pooled_observations(self):
        rows = [process(9, 100, observations=1000), process(2, 2, observations=1),
                process(5, 4, observations=1)]
        row = summarize(rows)[0]
        self.assertEqual(set(row), SUMMARY_KEYS | {"paired_ratio", "paired_change_percent"})
        self.assertEqual((row["median"], row["min"], row["max"]), (4, 2, 100))
        self.assertEqual((row["processes"], row["observations"]), (3, 1002))
        self.assertEqual(row["process_values"], [dict(process=2, value=2, observations=1),
                                                dict(process=5, value=4, observations=1),
                                                dict(process=9, value=100, observations=1000)])
        self.assertIsNone(row["paired_ratio"])

    def test_paired_ratios_match_indices_not_independent_medians(self):
        rows = [process(i, value, "baseline") for i, value in enumerate((1, 10, 100))]
        rows += [process(i, value) for i, value in enumerate((2, 30, 100))]
        result = summarize(list(reversed(rows)))
        candidate = next(r for r in result if r["variant"] == "candidate")
        baseline = next(r for r in result if r["variant"] == "baseline")
        self.assertEqual(set(baseline), SUMMARY_KEYS)
        self.assertEqual(candidate["paired_ratio"], 2)
        self.assertEqual(candidate["paired_change_percent"], 100)
        self.assertNotEqual(candidate["paired_ratio"], candidate["median"] / baseline["median"])

    def test_missing_or_zero_pairs_do_not_invent_infinity_or_select_partial_pairs(self):
        for rows in (
            [process(0, 0, "baseline"), process(0, 2)],
            [process(0, None, "baseline", 0), process(0, 2)],
            [process(0, 2, "baseline"), process(0, None, observations=0)],
            [process(0, 2, "baseline"), process(1, 3)],
            [process(0, 2, "baseline"), process(1, 0, "baseline"), process(0, 1), process(1, 1)],
        ):
            with self.subTest(rows=rows):
                for row in summarize(rows):
                    if row["variant"] == "candidate":
                        self.assertIsNone(row["paired_ratio"])
                        self.assertIsNone(row["paired_change_percent"])
        self.assertEqual(summarize([process(0, 1, "baseline"), process(0, 0)])[1]["paired_ratio"], 0)

    def test_unobserved_processes_are_retained_without_zero_imputation(self):
        row = summarize([process(0, None, observations=0), process(1, 10)])[0]
        self.assertEqual((row["median"], row["min"], row["max"], row["processes"]), (10, 10, 10, 2))
        self.assertIsNone(row["process_values"][0]["value"])
        row = summarize([process(0, None, observations=0)])[0]
        self.assertEqual((row["median"], row["min"], row["max"], row["observations"]), (None, None, None, 0))
        self.assertEqual(summarize([]), [])

    def test_duplicate_processes_metrics_and_schema_mismatches_fail(self):
        cases = []
        cases.append([process(0, 1), process(0, 2)])
        row = process(0, 1)
        row["metrics"] *= 2
        cases.append([row])
        for field, value in (("unit", "bytes"), ("direction", "neutral"), ("case", "other"), ("metric", "other")):
            row = process(1, 2)
            row["metrics"][0][field] = value
            cases.append([process(0, 1, "baseline"), row])
        row = process(1, 2)
        row["metrics"] = []
        cases.append([process(0, 1), row])
        for rows in cases:
            with self.subTest(rows=rows), self.assertRaises(EvidenceError):
                summarize(rows)

    def test_invalid_summary_values_and_observation_contract_fail(self):
        for value, observations in ((float("nan"), 11), (float("inf"), 11), (-1, 11),
                                    (True, 11), (1, 0), (None, 1), (1, -1), (1, True)):
            with self.subTest(value=value, observations=observations), self.assertRaises(EvidenceError):
                summarize([process(0, value, observations=observations)])
        row = process(0, 1)
        row["metrics"][0]["extra"] = 0
        with self.assertRaises(EvidenceError):
            summarize([row])


if __name__ == "__main__":
    unittest.main()
