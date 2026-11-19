import contextlib
import importlib.util
import io
import json
from pathlib import Path
import struct
import tempfile
import unittest
from unittest.mock import patch


spec = importlib.util.spec_from_file_location(
    "m68k_vectors", Path(__file__).parents[1] / "fetch-m68k-vectors.py"
)
vectors = importlib.util.module_from_spec(spec)
spec.loader.exec_module(vectors)


def state(**changes):
    result = {name: 0 for name in vectors.REGISTERS}
    result.update(sr=0x2000, pc=0x1004, prefetch=(0x4E71, 0xABCD), ram={})
    result.update(changes)
    return result


def section(magic, payload):
    return struct.pack("<II", len(payload) + 8, magic) + payload


def binary_state(value):
    payload = struct.pack("<19I", *(value[name] for name in vectors.REGISTERS))
    payload += struct.pack("<III", *value["prefetch"], len(value["ram"]) // 2)
    for address in sorted(value["ram"]):
        if address & 1 == 0:
            word = value["ram"][address] << 8 | value["ram"][address + 1]
            payload += struct.pack("<IH", address, word)
    return section(0x01234567, payload)


def binary_case(name, initial, final, transactions=(), total_cycles=None, durations=None):
    encoded = name.encode()
    payload = section(0x89ABCDEF, struct.pack("<I", len(encoded)) + encoded)
    payload += binary_state(initial) + binary_state(final)
    cycles = 4 * len(transactions) if total_cycles is None else total_cycles
    transfers = struct.pack("<II", cycles, len(transactions))
    for index, transaction in enumerate(transactions):
        duration = durations[index] if durations is not None else 4
        transfers += struct.pack("<BI", transaction[0], duration)
        if transaction[0]:
            transfers += struct.pack("<5I", *transaction[1:])
    payload += section(0x456789AB, transfers)
    return section(0xABC12367, payload)


def corpus(*records):
    return struct.pack("<II", 0x1A3F5D71, len(records)) + b"".join(records)


class BinaryIntakeTests(unittest.TestCase):
    def setUp(self):
        self.initial = state(ram={0x3000: 0x12, 0x3001: 0x34})
        self.raw = corpus(binary_case("one", self.initial, state(pc=0x1006)))

    def test_decodes_big_endian_word_memory_and_every_record(self):
        raw = corpus(
            binary_case("one", self.initial, state(pc=0x1006)),
            binary_case("two", state(), state(pc=0x1008)),
        )
        decoded = list(vectors.Decoder(raw).tests())
        self.assertEqual([record[0] for record in decoded], ["one", "two"])
        self.assertEqual(decoded[0][1]["ram"], {0x3000: 0x12, 0x3001: 0x34})

    def test_reference_instruction_total_is_record_local_and_not_transaction_sum(self):
        raw = corpus(
            binary_case("first", state(), state(), [(0,), (2, 6, 0x1004, 0, 1, 1)], total_cycles=34),
            binary_case("second", state(), state(), [(0,)], total_cycles=13),
        )
        decoder = vectors.Decoder(raw)
        observations = [(record[0], decoder.total_cycles, record[3]) for record in decoder.tests()]
        self.assertEqual([item[:2] for item in observations], [("first", 34), ("second", 13)])
        self.assertEqual(observations[0][2], [(2, 6, 0x1004, 0, 1, 1)])
        self.assertEqual(observations[1][2], [])

    def test_phase_decoder_retains_every_kind_duration_lane_and_record_boundary(self):
        transfers = [(0,)] + [(kind, 5, 0x3000, 0xAB00, 1, 0) for kind in range(1, 6)]
        durations = [2, 4, 7, 5, 0, 3]
        raw = corpus(binary_case("all kinds", state(), state(), transfers,
                                 total_cycles=40, durations=durations),
                     binary_case("next record", state(), state(), [(0,)],
                                 total_cycles=12, durations=[11]))
        decoder = vectors.Decoder(raw)
        records = decoder.tests()
        decoded = next(records)
        retained = decoder.phase_transactions
        self.assertEqual(decoded[3], transfers[1:])
        self.assertEqual(decoder.total_cycles, 40)
        self.assertNotEqual(sum(row["duration_cycles"] for row in retained), 40)
        self.assertEqual([row["kind"] for row in retained],
                         ["idle", "write", "read", "tas", "read_address_error", "write_address_error"])
        self.assertEqual([row["duration_cycles"] for row in retained], durations)
        self.assertEqual(retained[0], {"kind": "idle", "duration_cycles": 2})
        self.assertEqual(retained[-1], {"kind": "write_address_error", "duration_cycles": 3,
                                        "function_code": 5, "address_bus": 0x3000,
                                        "data_bus": 0xAB00, "uds": 1, "lds": 0})
        next(records)
        self.assertEqual(decoder.phase_transactions, [{"kind": "idle", "duration_cycles": 11}])
        self.assertEqual(len(retained), 6)
        with self.assertRaises(StopIteration):
            next(records)

    def test_rejects_unknown_file_and_nested_section_magic(self):
        for offset in (0, 12, 20):
            with self.subTest(offset=offset):
                damaged = bytearray(self.raw)
                struct.pack_into("<I", damaged, offset, 0)
                with self.assertRaises(ValueError):
                    list(vectors.Decoder(damaged).tests())

    def test_rejects_short_oversized_and_wrong_consumed_section_lengths(self):
        for length in (7, len(self.raw) + 1, len(self.raw) - 9):
            with self.subTest(length=length):
                damaged = bytearray(self.raw)
                struct.pack_into("<I", damaged, 8, length)
                with self.assertRaises(ValueError):
                    list(vectors.Decoder(damaged).tests())

    def test_rejects_truncation_and_trailing_bytes(self):
        for damaged in (self.raw[:-1], self.raw + b"\0", self.raw[:5]):
            with self.subTest(length=len(damaged)):
                with self.assertRaises((ValueError, struct.error)):
                    list(vectors.Decoder(damaged).tests())

    def test_rejects_record_count_that_hides_or_invents_records(self):
        for count in (0, 2):
            with self.subTest(count=count):
                damaged = bytearray(self.raw)
                struct.pack_into("<I", damaged, 4, count)
                with self.assertRaises((ValueError, struct.error)):
                    list(vectors.Decoder(damaged).tests())

    def test_cached_hash_mismatch_fails_without_network_fallback(self):
        with tempfile.TemporaryDirectory() as directory:
            cache = Path(directory)
            (cache / "source.bin").write_bytes(b"altered")
            with patch.object(vectors.urllib.request, "urlopen") as network:
                with self.assertRaisesRegex(ValueError, "cached source hash mismatch"):
                    vectors.fetch(cache, "source.bin", vectors.digest(self.raw))
            network.assert_not_called()


class ArchitecturalNormalizationTests(unittest.TestCase):
    def test_next_prefetch_pc_and_big_endian_instruction_seed(self):
        fixture, reason = vectors.normalize("NOP", state(), state(pc=0x1006), [])
        self.assertIsNone(reason)
        self.assertEqual(fixture["initial"]["pc"], 0x1000)
        self.assertEqual(fixture["final"]["pc"], 0x1002)
        self.assertEqual(dict(fixture["memory"]), {0x1000: 0x4E, 0x1001: 0x71,
                                                    0x1002: 0xAB, 0x1003: 0xCD})
        wrapped, _ = vectors.normalize("wrapped", state(pc=2), state(pc=4), [])
        self.assertEqual(wrapped["initial"]["pc"], 0xFFFFFFFE)
        self.assertEqual(dict(wrapped["memory"])[0xFFFFFE], 0x4E)

    def test_prefetch_disagreement_is_explicit(self):
        fixture, reason = vectors.normalize(
            "conflict", state(ram={0x1000: 0}), state(), []
        )
        self.assertIsNone(fixture)
        self.assertEqual(reason, "prefetch_ram_disagreement")

    def test_data_overlap_detected_even_before_later_prefetch_transaction(self):
        transfers = [(2, 1, 0x1008, 0x1234, 1, 1), (2, 2, 0x1008, 0x1234, 1, 1)]
        fixture, reason = vectors.normalize("overlap", state(), state(), transfers)
        self.assertIsNone(fixture)
        self.assertEqual(reason, "data_prefetch_overlap")

    def test_self_modifying_program_space_write_is_explicit(self):
        fixture, reason = vectors.normalize(
            "self-modifying", state(), state(), [(1, 6, 0x1000, 0x1234, 1, 1)]
        )
        self.assertIsNone(fixture)
        self.assertEqual(reason, "self_modifying_code")

    def test_byte_write_preserves_selected_lane_and_counts_only_zero_padding(self):
        for upper, lower, word, ram, selected, padding in (
            (0, 1, 0x007D, {0x3000: 0, 0x3001: 0x7D}, 0x3001, 0x3000),
            (1, 0, 0x7D00, {0x3000: 0x7D, 0x3001: 0}, 0x3000, 0x3001),
        ):
            with self.subTest(selected=selected):
                fixture, reason = vectors.normalize(
                    "byte", state(), state(ram=ram), [(1, 5, 0x3000, word, upper, lower)]
                )
                self.assertIsNone(reason)
                self.assertEqual(fixture["_unobserved_word_padding_bytes"], 1)
                self.assertEqual(dict(fixture["expected_memory"])[selected], 0x7D)
                self.assertNotIn(padding, dict(fixture["expected_memory"]))
                self.assertNotIn(selected, dict(fixture["memory"]))

    def test_existing_neighbor_remains_a_memory_witness(self):
        fixture, reason = vectors.normalize(
            "byte", state(ram={0x3000: 0x55}),
            state(ram={0x3000: 0x55, 0x3001: 0x7D}), [(1, 5, 0x3000, 0x7D, 0, 1)]
        )
        self.assertIsNone(reason)
        self.assertEqual(fixture["_unobserved_word_padding_bytes"], 0)
        self.assertEqual(dict(fixture["expected_memory"])[0x3000], 0x55)

    def test_final_only_nonzero_neighbor_and_missing_write_evidence_fail(self):
        cases = (
            (state(ram={0x3000: 1, 0x3001: 0x7D}), [(1, 5, 0x3000, 0x7D, 0, 1)]),
            (state(ram={0x3000: 0, 0x3001: 0x7D}), []),
            (state(ram={0x3000: 0, 0x3001: 0x7D}), [(2, 5, 0x3000, 0x7D, 0, 1)]),
        )
        for final, transactions in cases:
            with self.subTest(transactions=transactions):
                with self.assertRaisesRegex(ValueError, "unknown final-only memory convention"):
                    vectors.normalize("invalid neighbor", state(), final, transactions)

    def test_trace_and_both_address_fault_directions_have_named_exclusions(self):
        fixture, reason = vectors.normalize("trace", state(sr=0xA000), state(), [])
        self.assertIsNone(fixture)
        self.assertEqual(reason, "trace")
        for kind in (4, 5):
            with self.subTest(kind=kind):
                fixture, reason = vectors.normalize(
                    "address fault", state(), state(), [(kind, 5, 0x3001, 0, 1, 1)]
                )
                self.assertIsNone(fixture)
                self.assertEqual(reason, "address_fault")

    def test_low_supervisor_data_remains_excluded(self):
        fixture, reason = vectors.normalize(
            "low data", state(), state(), [(2, 5, 0x80, 0, 1, 1)]
        )
        self.assertIsNone(fixture)
        self.assertEqual(reason, "exception_or_low_supervisor_data")

    def test_trap_admits_only_its_exact_vector_words(self):
        initial = state(prefetch=(0x4E48, 0xABCD))
        transfers = [(2, 5, 160, 0, 1, 1), (2, 5, 162, 0x3000, 1, 1)]
        fixture, reason = vectors.normalize("TRAP 8", initial, state(pc=0x3004), transfers)
        self.assertIsNone(reason)
        self.assertEqual(fixture["final"]["pc"], 0x3000)
        for wrong_address in (128, 158, 164):
            with self.subTest(address=wrong_address):
                _, reason = vectors.normalize(
                    "wrong vector", initial, state(), [(2, 5, wrong_address, 0, 1, 1)]
                )
                self.assertEqual(reason, "exception_or_low_supervisor_data")

    def test_chk_divide_and_line_exceptions_admit_only_opcode_specific_vectors(self):
        for opcode, vector in ((0x4180, 6), (0x4580, 6), (0x80C0, 5),
                               (0x87C0, 5), (0xA123, 10), (0xFFFF, 11)):
            with self.subTest(opcode=opcode):
                address = vector * 4
                initial = state(prefetch=(opcode, 0xABCD),
                                ram={address: 0, address + 1: 0,
                                     address + 2: 0x50, address + 3: 0})
                transfers = [(2, 5, address, 0, 1, 1), (2, 5, address + 2, 0x5000, 1, 1)]
                fixture, reason = vectors.normalize("exception", initial, state(pc=0x5004), transfers)
                self.assertIsNone(reason)
                self.assertEqual(fixture["final"]["pc"], 0x5000)
                for unrelated in (address + 4, 32, 128):
                    _, reason = vectors.normalize(
                        "unrelated low data", initial, state(),
                        transfers + [(2, 5, unrelated, 0, 1, 1)]
                    )
                    self.assertEqual(reason, "exception_or_low_supervisor_data")
        _, reason = vectors.normalize("ordinary opcode", state(), state(), [(2, 5, 20, 0, 1, 1)])
        self.assertEqual(reason, "exception_or_low_supervisor_data")

    def test_reset_privilege_vector_requires_explicit_bus_selection_and_exact_opcode(self):
        initial = state(sr=0, prefetch=(0x4E70, 0xABCD),
                        ram={32: 0, 33: 0, 34: 0x50, 35: 0})
        transfers = [(2, 5, 32, 0, 1, 1), (2, 5, 34, 0x5000, 1, 1)]
        fixture, reason = vectors.normalize("RESET", initial, state(pc=0x5004), transfers)
        self.assertIsNone(fixture)
        self.assertEqual(reason, "exception_or_low_supervisor_data")
        fixture, reason = vectors.normalize("RESET", initial, state(pc=0x5004), transfers,
                                           bus_contracts=True)
        self.assertIsNone(reason)
        self.assertEqual(fixture["final"]["pc"], 0x5000)
        for unrelated in (20, 36, 128):
            with self.subTest(address=unrelated):
                _, reason = vectors.normalize("RESET", initial, state(),
                                              transfers + [(2, 5, unrelated, 0, 1, 1)],
                                              bus_contracts=True)
                self.assertEqual(reason, "exception_or_low_supervisor_data")
        _, reason = vectors.normalize("NOP", state(), state(), transfers, bus_contracts=True)
        self.assertEqual(reason, "exception_or_low_supervisor_data")
        _, reason = vectors.normalize("RESET trace", state(sr=0xA000, prefetch=(0x4E70, 0)),
                                      state(), transfers, bus_contracts=True)
        self.assertEqual(reason, "trace")
        _, reason = vectors.normalize("RESET address fault", initial, state(),
                                      [(4, 5, 32, 0, 1, 1)], bus_contracts=True)
        self.assertEqual(reason, "address_fault")


class CoverageLedgerTests(unittest.TestCase):
    def test_phase_export_preserves_excluded_records_raw_prefetch_and_clock_evidence(self):
        raw = corpus(
            binary_case("eligible", state(ram={0x3000: 0x12, 0x3001: 0x34}),
                        state(pc=0x1006, prefetch=(0xABCD, 0x1357)),
                        [(0,), (2, 6, 0x1004, 0x1357, 1, 1)], total_cycles=10, durations=[2, 4]),
            binary_case("trace", state(sr=0xA000), state(), [(0,)], total_cycles=12, durations=[6]),
            binary_case("address fault", state(), state(), [(4, 5, 0x3001, 0, 1, 1)],
                        total_cycles=50, durations=[0]),
        )
        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "phases.jsonl"
            with patch.object(vectors, "fetch", return_value=raw):
                receipt = vectors.export_phases(["NOP"], Path(directory), output)
            rows = [json.loads(line) for line in output.read_text().splitlines()]
            self.assertEqual(rows[0]["format"], "m68000-reference-phases-jsonl")
            self.assertEqual(rows[-1], {"type": "complete", "exported_records": 3})
            cases = rows[1:-1]
            self.assertEqual([case["name"] for case in cases], ["eligible", "trace", "address fault"])
            self.assertEqual([case["normalization_exclusion"] for case in cases], [None, "trace", "address_fault"])
            self.assertEqual(cases[0]["initial"]["pc"], 0x1004)
            self.assertEqual(cases[0]["initial"]["prefetch"], [0x4E71, 0xABCD])
            self.assertEqual(cases[0]["final"]["prefetch"], [0xABCD, 0x1357])
            self.assertEqual(cases[0]["initial"]["ram"], [[0x3000, 0x12], [0x3001, 0x34]])
            self.assertEqual([case["reference_total_cycles"] for case in cases], [10, 12, 50])
            self.assertEqual(cases[0]["transactions"][0], {"kind": "idle", "duration_cycles": 2})
            self.assertEqual(receipt["export_sha256"], vectors.digest(output.read_bytes()))
            self.assertEqual(receipt["cohorts"][0], {"cohort": "NOP", "raw_sha256": vectors.digest(raw),
                                                      "total": 3, "eligible": 1,
                                                      "excluded": {"address_fault": 1, "trace": 1}, "exported": 3})

    def test_phase_export_requires_explicit_cohorts_before_fetching(self):
        with patch("sys.argv", ["fetch", "--phase-export"]), \
                patch.object(vectors, "fetch") as fetch, contextlib.redirect_stderr(io.StringIO()):
            with self.assertRaises(SystemExit) as error:
                vectors.main()
        self.assertEqual(error.exception.code, 2)
        fetch.assert_not_called()

    def test_phase_export_default_path_never_overwrites_architectural_fixture(self):
        raw = corpus(binary_case("NOP", state(), state(pc=0x1006), total_cycles=4))

        def source(_cache, name, _expected=None):
            if name == "LICENSE":
                return b"MIT License"
            if name == "README.md":
                return b"+4 from where the test starts"
            return raw

        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cache = root / "rom-tests/cache/m68k"
            cache.mkdir(parents=True)
            previous = cache / "architectural-v1.json"
            previous.write_bytes(b"previous accepted fixture")
            with patch.object(vectors, "fetch", side_effect=source), \
                    patch("sys.argv", ["fetch", "--phase-export", "--cohorts", "NOP"]), \
                    patch.object(vectors, "__file__", str(root / "scripts/fetch.py")), \
                    contextlib.redirect_stdout(io.StringIO()):
                vectors.main()
            self.assertEqual(previous.read_bytes(), b"previous accepted fixture")
            self.assertTrue((cache / "reference-phases-v1.jsonl").exists())

    def test_m2b_families_do_not_collapse_into_immediate_or_existing_arithmetic(self):
        examples = {"MOVEM.w": 0x4890, "MOVEM.l": 0x4CD8, "MOVEP.w": 0x0108,
                    "MOVEP.l": 0x01C8, "ADDX.b": 0xD100, "SUBX.l": 0x9188,
                    "NEGX.w": 0x4040, "ABCD": 0xC100, "SBCD": 0x8108, "NBCD": 0x4800,
                    "MULU": 0xC0C0, "MULS": 0xC1C0, "DIVU": 0x80C0, "DIVS": 0x81C0,
                    "CHK": 0x4180, "BTST": 0x0800, "BCHG": 0x0140, "BCLR": 0x0880,
                    "BSET": 0x01C0, "ASL.w": 0xE3D0, "ASR.l": 0xE2A0, "LSL.b": 0xE308,
                    "LSR.w": 0xE2D0, "ROL.w": 0xE7D0, "ROR.l": 0xE2B8,
                    "ROXL.b": 0xE310, "ROXR.w": 0xE4D0, "RTR": 0x4E77,
                    "ILLEGAL_LINEA": 0xA123, "ILLEGAL_LINEF": 0xFFFF}
        for cohort, opcode in examples.items():
            with self.subTest(cohort=cohort):
                self.assertEqual(vectors.instruction_family(opcode, cohort), cohort.split(".")[0])
        self.assertEqual(len(vectors.BASE_COHORTS), 13)
        self.assertEqual(len(vectors.M2A_COHORTS), 71)
        self.assertEqual(len(vectors.M2B_COHORTS), 123)
        self.assertEqual(len(vectors.HASHES), 124)

    def test_bus_selection_and_default_paths_preserve_earlier_checkpoints(self):
        raw = corpus(binary_case("synthetic", state(), state(pc=0x1006), total_cycles=4))

        def source(_cache, name, _expected=None):
            if name == "LICENSE":
                return b"MIT License"
            if name == "README.md":
                return b"+4 from where the test starts"
            return raw

        self.assertNotIn("RESET", vectors.M2B_COHORTS)
        self.assertNotIn("RESET", vectors.M2A_COHORTS)
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for selection, suffix, count in (("--expanded", "expanded", 71),
                                             ("--m2b", "m2b", 123),
                                             ("--bus-contracts", "bus", 124)):
                for nominal in (False, True):
                    args = ["fetch", selection] + (["--nominal-cycles"] if nominal else [])
                    with patch.object(vectors, "fetch", side_effect=source), patch("sys.argv", args), \
                            patch.object(vectors, "__file__", str(root / "scripts/fetch.py")), \
                            contextlib.redirect_stdout(io.StringIO()):
                        vectors.main()
                    prefix = "nominal" if nominal else "architectural"
                    output = root / "rom-tests/cache/m68k" / f"{prefix}-{suffix}-v1.json"
                    receipt = json.loads(output.with_suffix(".receipt.json").read_text())
                    self.assertEqual(len(receipt["cohorts"]), count)
                    included = {cohort["cohort"] for cohort in receipt["cohorts"]}
                    self.assertEqual("RESET" in included, selection == "--bus-contracts")
                    self.assertIn("not selected", receipt["coverage_limits"]["TAS"])
                    if selection == "--bus-contracts":
                        self.assertIn("pulse output/order not witnessed", receipt["coverage_limits"]["RESET"])

    def test_nominal_option_is_explicit_and_preserves_reference_totals_and_admission(self):
        raw = corpus(
            binary_case("first", state(), state(pc=0x1006), [(0,)], total_cycles=4),
            binary_case("second", state(), state(pc=0x1006), [(0,)], total_cycles=34),
        )

        def source(_cache, name, _expected=None):
            if name == "LICENSE":
                return b"MIT License"
            if name == "README.md":
                return b"+4 from where the test starts"
            return raw

        outputs = []
        with tempfile.TemporaryDirectory() as directory:
            for nominal in (False, True):
                output = Path(directory) / ("nominal.json" if nominal else "architecture.json")
                args = ["fetch", "--cohorts", "NOP", "--output", str(output)]
                if nominal:
                    args.append("--nominal-cycles")
                with patch.object(vectors, "fetch", side_effect=source), patch("sys.argv", args), \
                        contextlib.redirect_stdout(io.StringIO()):
                    vectors.main()
                payload = json.loads(output.read_text())
                receipt = json.loads(output.with_suffix(".receipt.json").read_text())
                self.assertEqual(receipt["nominal_cycle_comparison"], nominal)
                self.assertEqual(receipt["total_eligible"], 2)
                self.assertIn("no per-transfer timing" if nominal else "no cycles", receipt["claim"])
                self.assertEqual(receipt["normalized_sha256"], vectors.digest(output.read_bytes()))
                if nominal:
                    self.assertEqual([case.pop("expected_nominal_cycles") for case in payload["cases"]], [4, 34])
                else:
                    self.assertTrue(all("expected_nominal_cycles" not in case for case in payload["cases"]))
                outputs.append(payload)
        self.assertEqual(outputs[0], outputs[1])

    def test_cmpm_is_distinct_from_cmp_and_eor_in_opcode_coverage(self):
        for opcode, cohort in ((0xB108, "CMP.b"), (0xB148, "CMP.w"), (0xB188, "CMP.l"),
                               (0xBF4D, "CMP.w")):
            with self.subTest(opcode=opcode):
                self.assertEqual(vectors.instruction_family(opcode, cohort), "CMPM")
        self.assertEqual(vectors.instruction_family(0xB010, "CMP.b"), "CMP")
        self.assertEqual(vectors.instruction_family(0xB100, "EOR.b"), "EOR")
        self.assertEqual(vectors.instruction_family(0xB1C8, "CMPA.l"), "CMPA")

    def test_cli_processes_every_record_and_reconciles_exclusions_without_download(self):
        raw = corpus(
            binary_case("admitted", state(), state(pc=0x1006)),
            binary_case("trace", state(sr=0xA000), state()),
            binary_case("address", state(), state(), [(4, 5, 0x3001, 0, 1, 1)]),
            binary_case("last admitted", state(), state(pc=0x1006)),
        )

        def source(_cache, name, _expected=None):
            if name == "LICENSE":
                return b"MIT License"
            if name == "README.md":
                return b"+4 from where the test starts"
            return raw

        with tempfile.TemporaryDirectory() as directory:
            output = Path(directory) / "fixture.json"
            with patch.object(vectors, "fetch", side_effect=source), \
                    patch("sys.argv", ["fetch", "--cohorts", "NOP", "--output", str(output)]), \
                    contextlib.redirect_stdout(io.StringIO()):
                vectors.main()
            receipt = json.loads(output.with_suffix(".receipt.json").read_text())
            payload = json.loads(output.read_text())
            self.assertEqual([case["name"] for case in payload["cases"]],
                             ["admitted", "last admitted"])
            self.assertEqual(receipt["total_eligible"], 2)
            self.assertEqual(receipt["normalized_sha256"], vectors.digest(output.read_bytes()))
            cohort = receipt["cohorts"][0]
            self.assertEqual(cohort["total"], 4)
            self.assertEqual(cohort["eligible"], 2)
            self.assertEqual(cohort["excluded"], {"address_fault": 1, "trace": 1})
            self.assertEqual(cohort["opcode_families"], {"NOP": {"total": 4, "eligible": 2}})


if __name__ == "__main__":
    unittest.main()
