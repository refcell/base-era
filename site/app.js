const routes = {
  19: {
    label: "BLOCK 19 · WORKER ROUTE",
    title: "Holocene rules, isolated",
    copy: "The host binds the request to block 19’s exact parent and sends complete-block execution to the approved historical worker.",
    executor: "Pinned worker",
    state: "Immutable parent view",
  },
  20: {
    label: "BLOCK 20 · LOCAL ROUTE",
    title: "Isthmus activates here",
    copy: "Block 20 has a pre-Isthmus parent. The real activation injects upgrade deposits and changes commitment rules; current code executes it in-process.",
    executor: "Current host",
    state: "Canonical state",
  },
  21: {
    label: "BLOCK 21 · LOCAL ROUTE",
    title: "Current rules continue",
    copy: "With Isthmus active, block 21 remains on the host’s current execution path. The host still owns validation, forkchoice and persistence.",
    executor: "Current host",
    state: "Canonical state",
  },
};

document.querySelectorAll(".block").forEach((button) =>
  button.addEventListener("click", () => {
    document.querySelectorAll(".block").forEach((item) => {
      item.classList.toggle("active", item === button);
      item.setAttribute("aria-pressed", item === button);
    });
    const route = routes[button.dataset.block];
    document.querySelector("#route-label").textContent = route.label;
    document.querySelector("#route-title").textContent = route.title;
    document.querySelector("#route-copy").textContent = route.copy;
    document.querySelector("#route-executor").textContent = route.executor;
    document.querySelector("#route-state").textContent = route.state;
  }),
);

document.querySelector("#copy").addEventListener("click", async (event) => {
  const button = event.currentTarget;
  try {
    await navigator.clipboard.writeText(
      document.querySelector("#commands").textContent,
    );
    button.textContent = "Copied";
  } catch {
    button.textContent = "Select + copy";
  }
  setTimeout(() => {
    button.textContent = "Copy commands";
  }, 1800);
});

function renderEvidence(data) {
  const migrated = data.provenance.status === "migrated-pass";
  const provenanceName = migrated ? "Migrated acceptance" : "Source spike";

  document.querySelector("#evidence-label").textContent = data.provenance.label;
  document.querySelector("#evidence-date").textContent =
    `${migrated ? "Acceptance run" : "Final local run"} · ${data.provenance.date}`;
  document.querySelector("#checkout-status").textContent =
    data.provenance.checkoutStatus;
  document.querySelector("#hero-provenance").textContent =
    `${provenanceName} · ${data.provenance.date}`;
  document.querySelector("#footer-provenance").textContent =
    `${provenanceName} · ${data.provenance.date.slice(0, 4)}`;
  document.querySelector("#header-status").textContent = migrated
    ? "Acceptance passed"
    : "Source spike";
  document.querySelector("#demo-action").innerHTML = migrated
    ? 'Run the demo <span aria-hidden="true">→</span>'
    : 'Review the demo commands <span aria-hidden="true">→</span>';
  document.querySelector("#quickstart-status").textContent = migrated
    ? "Verified demo interface · repository root"
    : "Planned demo interface · repository root";
  document.querySelector("#quickstart-note").innerHTML = migrated
    ? "<strong>Acceptance passed.</strong> These commands reproduce the migrated demo from the repository root."
    : "<strong>These commands are the planned interface.</strong> They become runnable when the demo migration lands; this evidence does not claim a successful migrated run.";
  document.querySelector("#source-note").innerHTML = migrated
    ? `Figures above come from the migrated acceptance run described by <code>site/data/evidence.json</code> (${data.provenance.environment}).`
    : "Figures above come from the disposable source spike, not CI or a fresh run of this checkout. Replace <code>site/data/evidence.json</code> with migrated-run results only after acceptance passes.";

  document.querySelector("#stats").innerHTML = data.checks
    .map(
      (check) =>
        `<div class="stat"><b>${check.value}</b><span>${check.label}</span><small>${check.detail}</small></div>`,
    )
    .join("");
  document.querySelector("#worker-latency").textContent =
    `${data.latency.workerMs} ms`;
  document.querySelector("#reference-latency").textContent =
    `${data.latency.referenceMs} ms`;
  document
    .querySelector("#reference-bar")
    .style.setProperty(
      "--size",
      `${(data.latency.referenceMs / data.latency.workerMs) * 100}%`,
    );
  document.querySelector("#latency-note").textContent = data.latency.note;
}

fetch("./data/evidence.json")
  .then((response) => {
    if (!response.ok) throw new Error("Evidence unavailable");
    return response.json();
  })
  .then(renderEvidence)
  .catch(() => {
    document.querySelector("#stats").innerHTML =
      "<p>Evidence data unavailable. Serve the site over HTTP to load it.</p>";
  });
