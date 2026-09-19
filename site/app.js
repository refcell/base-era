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
    button.textContent = "Copy command";
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
          `<span><strong>${check.value}</strong> ${check.label} <small>${check.detail}</small></span>`,
      )
      .join("");
    document.querySelector("#benchmarks").innerHTML = data.performance
      .slice(0, 4)
      .map(
        (row) =>
          `<tr${row.historical ? ' class="historical"' : ""}><th scope="row">${row.metric}</th><td>${row.host}</td><td>${row.reference}</td></tr>`,
      )
      .join("");
    document.querySelector("#provenance").textContent =
      `${data.provenance.date} · Recorded local acceptance run. Full results and methodology in the raw evidence.`;
    document.querySelector("#header-status").textContent =
      data.provenance.status === "migrated-pass"
        ? "Live demo acceptance passed"
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

fetch("./data/historical-call.json")
  .then((response) => {
    if (!response.ok) throw new Error("Call evidence unavailable");
    return response.json();
  })
  .then((call) => {
    document.querySelector("#call-context").textContent =
      `eth_call · block ${call.blockNumber} · ${call.era} · GasPriceOracle.isIsthmus() → false`;
    document.querySelector("#http-time").textContent =
      `${call.httpMs.toFixed(3)} ms`;
    document.querySelector("#worker-time").textContent =
      `${call.workerMs.toFixed(3)} ms`;
    const bar = document.querySelector("#worker-bar");
    bar.style.marginLeft = `${(call.workerOffsetMs / call.httpMs) * 100}%`;
    bar.style.width = `${(call.workerMs / call.httpMs) * 100}%`;
    document.querySelector("#call-detail").textContent =
      `Worker PID ${call.workerPid} · ${call.stateReads} reads · ${call.readBytes.toLocaleString("en-US")} read-request bytes · ${call.requestBytes.toLocaleString("en-US")} byte operation request · artifact ${call.artifact.slice(0, 14)}… · sequencer/verifier ${call.builderVerifierMatch ? "hash + state match" : "MISMATCH"}`;
  })
  .catch(() => {
    document.querySelector("#call-context").textContent =
      "Call evidence unavailable. Open the captured request and logs directly.";
    document.querySelector("#waterfall").classList.add("unavailable");
  });
