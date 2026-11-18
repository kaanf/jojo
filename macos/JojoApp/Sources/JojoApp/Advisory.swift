import JojoCore
import SwiftUI

// Protocol 1.14 (follow-ups): the advisory review of a candidate started by hand
// (Verification & Evidence) or of a drafted document (Specifications, Approval Inbox). The
// provider that did not do the work reads it once, read-only; its verdict approves nothing,
// rejects nothing and holds nothing back. One section shows the newest review and asks for
// another.

enum Advisory {
    /// Why a review cannot be asked for now, or nil when it can. `offer` nil: the core did
    /// not report one (an older helper).
    static func blocked(link: RecoveryCenterState.Link, commandPending: Bool, offer: AdvisoryOffer?) -> String? {
        if link != .connected { return "Unavailable: the core is not connected." }
        guard let offer else { return "This core reports no advisory review." }
        if offer.review?.running == true { return "The review is running." }
        if commandPending { return "Unavailable while a command awaits the core." }
        if !offer.available { return offer.reason ?? "The core offers no review of this." }
        return nil
    }

    /// The pill of a review's state.
    static func pill(_ state: String) -> (String, Tone) {
        switch state {
        case "Approved": return ("Would approve", .ok)
        case "Rejected": return ("Objects", .warn)
        case "Running": return ("Reviewing", .info)
        case "Stopping": return ("Stopping", .neutral)
        case "Cancelled": return ("Stopped", .neutral)
        case "Interrupted": return ("Interrupted", .neutral)
        default: return ("Failed", .danger)
        }
    }

    static func provider(_ word: String?) -> String {
        word == "ClaudeCode" ? "Claude Code" : word ?? "The other provider"
    }
}

struct AdvisorySection: View {
    @Environment(\.theme) private var theme
    let offer: AdvisoryOffer?
    /// "this candidate", "this revision".
    let subject: String
    let blocked: String?
    let request: () -> Void

    var body: some View {
        LabeledSection(title: "Second opinion") {
            if let review = offer?.review {
                DefinitionCard(rows: [
                    Definition(key: "Reviewer", value: Advisory.provider(review.provider), pill: Advisory.pill(review.state)),
                ] + (review.finished.map { [Definition(key: "Finished", value: $0.formatted(date: .abbreviated, time: .shortened))] } ?? []),
                keyWidth: 80)
                if !review.notes.isEmpty {
                    Text(review.notes).font(.system(size: 12)).foregroundStyle(theme.text)
                        .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                }
                if let objection = review.objection {
                    Text("Strongest objection: \(objection)").font(.system(size: 12)).foregroundStyle(theme.text2)
                        .fixedSize(horizontal: false, vertical: true)
                }
                if !review.detail.isEmpty, review.notes.isEmpty {
                    Text(review.detail).textStyle(.mono).foregroundStyle(theme.text2)
                        .fixedSize(horizontal: false, vertical: true).textSelection(.enabled)
                }
            }
            Button(offer?.review == nil ? "Request Review" : "Request Another Review", action: request)
                .buttonStyle(.jojo(.secondary)).frame(maxWidth: .infinity)
                .disabled(blocked != nil)
            if let blocked { DisabledReason(blocked) }
            Text("\(Advisory.provider(offer?.reviewer ?? offer?.review?.provider)) reads \(subject) read-only. Its verdict approves nothing and holds nothing back; you still decide.")
                .font(.system(size: 11)).foregroundStyle(theme.text3).fixedSize(horizontal: false, vertical: true)
        }
    }
}
