"""R-01: the helper's request table (Rust) and the client's request list (Swift) agree."""
import re
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
RUST = ROOT / 'crates/jojo-domain/src/protocol.rs'
SWIFT = ROOT / 'macos/JojoApp/Sources/JojoCore/Protocol.swift'
SWIFT_SOURCES = ROOT / 'macos/JojoApp/Sources'

# Requests the helper serves that this client never sends. Adding a request to the helper
# means adding it to the client or naming it here.
NOT_SENT_BY_THE_CLIENT = {'RegisterRepository', 'ImportBundle', 'EventDetail', 'ShutdownReceipt'}

SPEC = re.compile(r'spec\(K::(\w+), "(\w+)", (\w+), (\d+), (true|false), (\w+), &\[([^\]]*)\]\)')
CASE = re.compile(r'case \.(\w+): return \[([^\]]*)\]')


def names(listed):
    return re.findall(r'"(\w+)"', listed)


def rust_table(source):
    return {name: {'kind': kind, 'class': cls, 'since': int(since), 'shape': shape, 'fields': names(fields)}
            for kind, name, cls, since, _, shape, fields in SPEC.findall(source)}


def swift_table(source):
    return {name: names(fields) for name, fields in CASE.findall(source)}


def swift_cases(source):
    declared = re.search(r'enum Request: String, CaseIterable, Sendable \{(.*?)public var fields', source, re.S).group(1)
    return [name for line in re.findall(r'case ([\w, ]+)\n', declared) for name in line.split(', ')]


class ProtocolFields(unittest.TestCase):
    def setUp(self):
        self.rust_source = RUST.read_text()
        self.swift_source = SWIFT.read_text()
        self.rust = rust_table(self.rust_source)
        self.swift = swift_table(self.swift_source)

    def test_the_rust_table_is_read_completely(self):
        declared = int(re.search(r'pub const SPECS: \[Spec; (\d+)\]', self.rust_source).group(1))
        self.assertEqual(len(self.rust), declared)
        self.assertEqual(self.rust_source.count('spec(K::'), declared)
        variants = re.search(r'pub enum RequestKind \{(.*?)\}', self.rust_source, re.S).group(1)
        self.assertEqual(re.findall(r'(\w+),', variants), list(self.rust))
        for name, spec in self.rust.items():
            self.assertEqual(spec['kind'], name)

    def test_the_swift_list_is_read_completely(self):
        cases = swift_cases(self.swift_source)
        self.assertEqual(sorted(cases), sorted(self.swift))
        self.assertEqual(len(cases), len(set(cases)))
        self.assertEqual(len(re.findall(r'case \.\w', self.swift_source)), len(self.swift))

    def test_every_client_request_has_the_helpers_fields_in_the_helpers_order(self):
        for name, fields in self.swift.items():
            self.assertIn(name, self.rust)
            self.assertEqual(fields, self.rust[name]['fields'], name)

    def test_helper_requests_the_client_does_not_send_are_named(self):
        self.assertEqual(set(self.rust) - set(self.swift), NOT_SENT_BY_THE_CLIENT)

    def test_a_difference_in_either_table_is_detected(self):
        changed = self.swift_source.replace('case .Start: return ["prd", "adr"', 'case .Start: return ["adr", "prd"')
        self.assertNotEqual(changed, self.swift_source)
        self.assertNotEqual(swift_table(changed)['Start'], self.rust['Start']['fields'])
        changed = self.rust_source.replace('&["digest", "attempt", "choice"]', '&["digest", "attempt", "choice", "force"]')
        self.assertNotEqual(changed, self.rust_source)
        self.assertNotEqual(rust_table(changed)['RecoveryResolve']['fields'], self.swift['RecoveryResolve'])

    def test_the_client_sends_requests_only_through_the_typed_list(self):
        sent = re.compile(r'\b(?:query|command|slice|loadText)\(\s*"')
        for path in sorted(SWIFT_SOURCES.rglob('*.swift')):
            self.assertIsNone(sent.search(path.read_text()), f'{path.relative_to(ROOT)} sends a request by a string name')

    def test_the_client_does_not_match_on_assessment_text(self):
        for path in sorted(SWIFT_SOURCES.rglob('*.swift')):
            for line in path.read_text().splitlines():
                if 'assessment' in line:
                    self.assertIsNone(re.search(r'assessment\??\.(?:contains|hasPrefix|hasSuffix|range|split|firstIndex)', line),
                                      f'{path.relative_to(ROOT)}: {line.strip()}')


if __name__ == '__main__':
    unittest.main()
