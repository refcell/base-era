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

fetch("./data/evidence.json")
  .then((response) => {
    if (!response.ok) throw new Error("Evidence unavailable");
    return response.json();
  })
  .then((data) => {
    document.querySelector("#stats").innerHTML = data.checks
      .map(
        (check) =>
          `<div class="stat"><b>${check.value}</b><span>${check.label}</span><small>${check.detail}</small></div>`,
      )
      .join("");
    document.querySelector("#benchmarks").innerHTML = data.performance
      .map(
        (row) =>
          `<tr${row.historical ? ' class="historical"' : ""}><th scope="row">${row.metric}</th><td>${row.host}</td><td>${row.reference}</td></tr>`,
      )
      .join("");
    document.querySelector("#provenance").textContent =
      `${data.provenance.date} · ${data.provenance.checkoutStatus}. Recorded local run, not CI or a live benchmark.`;
    document.querySelector("#measurement-note").textContent =
      `${data.provenance.environment}. ${data.latency.note} CPU/RSS: wait4, including waited-for descendants; RSS is not summed.`;
    document.querySelector("#header-status").textContent =
      data.provenance.status === "migrated-pass"
        ? "Acceptance passed"
        : "Source-spike evidence";
  })
  .catch(() => {
    document.querySelector("#header-status").textContent =
      "Evidence unavailable";
    document.querySelector("#stats").textContent =
      "Could not load results. Use the raw evidence link above.";
    document.querySelector("#benchmarks").innerHTML =
      '<tr><td colspan="3">Measurements unavailable.</td></tr>';
  });
