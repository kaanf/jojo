import Foundation

// Protocol 1.11 (step 6): what the Recovery Center, Projects and Activity & Audit read
// beyond the earlier queries. Everything is derived by the core; nothing is stored.

/// The answer of `RecoveryDetail`: the inspection the helper holds, counted and itemized.
public struct RecoveryDetail: Equatable, Sendable {
    public struct Counts: Equatable, Sendable {
        public let examined, worktrees, worktreesIntact, snapshotMismatches: Int64
        public let unknownEffects, staleApprovals, evidenceGaps, claims: Int64

        init(_ p: JSON?) {
            let n = { (k: String) in p?[k]?.integer ?? 0 }
            (examined, worktrees, worktreesIntact, snapshotMismatches) =
                (n("examined"), n("worktrees"), n("worktrees_intact"), n("snapshot_mismatches"))
            (unknownEffects, staleApprovals, evidenceGaps, claims) =
                (n("unknown_effects"), n("stale_approvals"), n("evidence_gaps"), n("claims"))
        }
    }

    /// A state word and what it adds: a process finding, the worktree, the snapshot, the
    /// provider session.
    public struct Fact: Equatable, Sendable {
        public let label: String
        public let state: String
        public let detail: String

        init(label: String = "", _ p: JSON?, state key: String = "state", detail: String = "detail") {
            self.label = label.isEmpty ? (p?["label"]?.string ?? "") : label
            state = p?[key]?.string ?? "Unknown"
            self.detail = p?[detail]?.string ?? ""
        }
    }

    public struct Claim: Equatable, Sendable, Identifiable {
        public let resource: String
        public let mode: String
        /// `ACTIVE` or `QUARANTINED` for a subject's claim; nil for another holder's.
        public let state: String?
        /// "T4 · attempt 1" for a claim another attempt holds.
        public let holder: String?
        public var id: String { resource + mode + (holder ?? "") }

        init(_ p: JSON) {
            resource = p["resource"]?.string ?? ""
            mode = p["mode"]?.string ?? ""
            state = p["state"]?.string
            holder = p["holder_label"]?.string ?? p["holder"]?.string.map { "\(p["holder_kind"]?.string ?? "holder") \($0.prefix(8))" }
        }
    }

    public struct Subject: Equatable, Sendable, Identifiable {
        public let attempt: String
        /// "T4 · attempt 2"; nil for an attempt without a step.
        public let label: String?
        public let title: String?
        public let featureName: String?
        public let projectName: String?
        public let repositoryName: String?
        public let ordinal: Int64?
        /// `NotIssued`, `IssuedWithoutIdentity`, `Spawned`, `Concluded`, `Sealed`.
        public let stage: String
        public let processes: [Fact]
        public let worktree: Fact
        public let snapshot: Fact
        public let snapshotDigest: String?
        public let evidenceGaps: [String]
        public let staleApprovals: [String]
        public let unknownEffects: [String]
        public let session: Fact
        public let claims: [Claim]
        public let blockers: [String]
        public let processAbsent: Bool
        public let choices: [RecoveryChoice]
        public var id: String { attempt }

        init?(_ p: JSON) {
            guard let attempt = p["attempt"]?.string, let stage = p["stage"]?.string else { return nil }
            (self.attempt, self.stage) = (attempt, stage)
            label = p["label"]?.string
            title = p["title"]?.string
            featureName = p["feature_name"]?.string
            projectName = p["project_name"]?.string
            repositoryName = p["repository_name"]?.string
            ordinal = p["ordinal"]?.integer
            processes = (p["processes"]?.items ?? []).map { Fact($0, state: "finding") }
            worktree = Fact(label: "Worktree", p["worktree"])
            snapshot = Fact(label: "Snapshot", p["snapshot"])
            snapshotDigest = p["snapshot"]?["digest"]?.string
            let texts = { (k: String) in (p[k]?.items ?? []).compactMap(\.string) }
            (evidenceGaps, staleApprovals, unknownEffects) = (texts("evidence_gaps"), texts("stale_approvals"), texts("unknown_effects"))
            session = Fact(label: "Session", p["session"])
            claims = (p["claims"]?.items ?? []).map(Claim.init)
            blockers = texts("blockers")
            processAbsent = p["process_absent"]?.boolean ?? false
            choices = texts("choices").compactMap(RecoveryChoice.init(rawValue:))
        }
    }

    public struct Settled: Equatable, Sendable, Identifiable {
        public let attempt: String
        public let label: String?
        /// `VALIDATE_OUTPUT`, `CONTINUE_WITH_HISTORY`, `CONTINUE_FRESH_CONTEXT` or `HOLD`.
        public let choice: String
        public let continued: Bool
        public let at: Date?
        public var id: String { attempt + choice + "\(at?.timeIntervalSince1970 ?? 0)" }

        init?(_ p: JSON) {
            guard let attempt = p["attempt"]?.string, let choice = p["choice"]?.string else { return nil }
            (self.attempt, self.choice) = (attempt, choice)
            label = p["label"]?.string
            continued = p["continued"]?.boolean ?? false
            at = helperDate(p["at_unix_ms"]?.integer)
        }
    }

    public let digest: String
    public let inspectedAt: Date?
    public let counts: Counts
    public let subjects: [Subject]
    public let claims: [Claim]
    public let settled: [Settled]

    public init?(_ p: [String: JSON]) {
        guard let digest = p["digest"]?.string, let rows = p["subjects"]?.items else { return nil }
        self.digest = digest
        inspectedAt = helperDate(p["inspected_at_unix_ms"]?.integer)
        counts = Counts(p["counts"])
        subjects = rows.compactMap(Subject.init)
        claims = (p["claims"]?.items ?? []).map(Claim.init)
        settled = (p["settled"]?.items ?? []).compactMap(Settled.init)
    }

    public func subject(_ attempt: String) -> Subject? { subjects.first { $0.attempt == attempt } }
}

/// The answer of `ProjectDetail`: one project's repositories as the core reads them now.
public struct ProjectDetail: Equatable, Sendable {
    public struct Repository: Equatable, Sendable, Identifiable {
        public let id: String
        public let name: String
        public let path: String?
        public let branch: String?
        public let head: String?
        public let origin: String?
        /// `Https`, `Ssh`, `File` or `Other`.
        public let originKind: String?
        public let worktrees: Int64
        public let activeWorktrees: Int64
        public let healthy: Bool
        public let issues: [String]

        init?(_ p: JSON) {
            guard let id = p["repository"]?.string else { return nil }
            self.id = id
            name = p["name"]?.string ?? ""
            path = p["path"]?.string
            branch = p["branch"]?.string
            head = p["head"]?.string
            origin = p["origin"]?.string
            originKind = p["origin_kind"]?.string
            worktrees = p["worktrees"]?.integer ?? 0
            activeWorktrees = p["active_worktrees"]?.integer ?? 0
            healthy = p["healthy"]?.boolean ?? false
            issues = (p["issues"]?.items ?? []).compactMap(\.string)
        }
    }

    public struct Missing: Equatable, Sendable, Identifiable {
        public let repository: String?
        public let text: String
        public var id: String { text }
    }

    public let project: String
    public let revision: Int64
    public let name: String
    public let repositories: [Repository]
    public let missing: [Missing]

    public init?(_ p: [String: JSON]) {
        guard let project = p["project"]?.string, let rows = p["repositories"]?.items else { return nil }
        self.project = project
        revision = p["revision"]?.integer ?? 0
        name = p["name"]?.string ?? ""
        repositories = rows.compactMap(Repository.init)
        missing = (p["missing"]?.items ?? []).compactMap { m in
            m["text"]?.string.map { Missing(repository: m["repository"]?.string, text: $0) }
        }
    }
}

/// The whole payload of one event, read with `EventDetail` in pages of 2,048 characters.
public struct EventPayload: Equatable, Sendable {
    public let seq: Int64
    public let text: String
    public let total: Int64
    public var complete: Bool { Int64(text.count) >= total }
}

extension WorkspaceModel {
    /// The inspection the helper holds with its counts and facts (protocol 1.11). Read
    /// only while the Recovery Center is the shown screen.
    public func loadRecoveryDetail() {
        guard recoveryDetailLoad != .loading, snapshotSeq != nil else { return }
        recoveryDetailLoad = .loading
        onChange?()
        client.query(.RecoveryDetail, [:]) { [weak self] reply in
            guard let self else { return }
            switch reply {
            case .response(let p):
                if let detail = RecoveryDetail(p) {
                    (self.recoveryDetail, self.recoveryDetailLoad) = (detail, .loaded)
                } else {
                    self.recoveryDetailLoad = .failed("malformed recovery detail")
                }
            case .error(let reason), .unanswered(let reason):
                self.recoveryDetailLoad = .failed(reason)
            default:
                self.recoveryDetailLoad = .failed("unexpected answer")
            }
            self.onChange?()
        }
    }

    /// One project's repositories (protocol 1.11). Read only while Projects is shown.
    public func loadProjectDetail(_ project: String) {
        guard projectDetailKey != project || projectDetailLoad != .loading else { return }
        projectDetailKey = project
        projectDetailLoad = .loading
        onChange?()
        client.query(.ProjectDetail, ["project": .text(project)]) { [weak self] reply in
            guard let self, self.projectDetailKey == project else { return }
            switch reply {
            case .response(let p):
                if let detail = ProjectDetail(p) {
                    (self.projectDetail, self.projectDetailLoad) = (detail, .loaded)
                } else {
                    (self.projectDetail, self.projectDetailLoad) = (nil, .failed("malformed project detail"))
                }
            case .null:
                (self.projectDetail, self.projectDetailLoad) = (nil, .failed("This core has no such project."))
            case .error(let reason), .unanswered(let reason):
                (self.projectDetail, self.projectDetailLoad) = (nil, .failed(reason))
            default:
                (self.projectDetail, self.projectDetailLoad) = (nil, .failed("unexpected answer"))
            }
            self.onChange?()
        }
    }

    /// The rest of a long event payload, page after page (`EventDetail`, protocol 1.0).
    /// Read only when the human asks for it.
    public func loadEventPayload(_ seq: Int64) {
        guard eventPayloadKey != seq || eventPayloadLoad != .loading else { return }
        eventPayloadKey = seq
        eventPayloadLoad = .loading
        eventPayload = nil
        onChange?()
        readPayload(seq, offset: 0, collected: "")
    }

    private func readPayload(_ seq: Int64, offset: Int64, collected: String) {
        client.query(.EventDetail, ["seq": .int(seq), "offset": .int(offset)]) { [weak self] reply in
            guard let self, self.eventPayloadKey == seq else { return }
            switch reply {
            case .response(let p):
                guard let text = p["text"]?.string, let next = p["next"]?.integer, let total = p["total"]?.integer else {
                    self.eventPayloadLoad = .failed("malformed event payload")
                    break
                }
                let all = collected + text
                // A bounded read: at most 32 pages (64 KiB of text).
                if next < total, !text.isEmpty, offset / 2048 < 31 {
                    return self.readPayload(seq, offset: next, collected: all)
                }
                (self.eventPayload, self.eventPayloadLoad) = (EventPayload(seq: seq, text: all, total: total), .loaded)
            case .null:
                self.eventPayloadLoad = .failed("This core has no such event.")
            case .error(let reason), .unanswered(let reason):
                self.eventPayloadLoad = .failed(reason)
            default:
                self.eventPayloadLoad = .failed("unexpected answer")
            }
            self.onChange?()
        }
    }
}
