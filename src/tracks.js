/* Pintado de la lista de pistas. Vive aparte porque lo usan las dos ventanas:
   la principal no lo usa ya, pero la de detalle sí, y el criterio de nombres y
   iconos tiene que ser el mismo en ambas. */

/** ffmpeg usa identificadores largos; la gente conoce los nombres cortos. */
const CODEC_NAME = {
  hdmv_pgs_subtitle: "pgs",
  dvd_subtitle: "vobsub",
  dvb_subtitle: "dvbsub",
  subrip: "srt",
  eac3: "e-ac3",
  pcm_s16le: "pcm",
  pcm_s24le: "pcm",
};

export const codecName = (c) => CODEC_NAME[c] ?? c;

/* Iconos de lucide. Van en gris neutro: el color ya significa la decisión. */
const KIND_ICON = {
  video: '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M7 3v18"/><path d="M3 7.5h4"/>'
       + '<path d="M3 12h18"/><path d="M3 16.5h4"/><path d="M17 3v18"/><path d="M17 7.5h4"/><path d="M17 16.5h4"/>',
  audio: '<path d="M2 10v3"/><path d="M6 6v11"/><path d="M10 3v18"/><path d="M14 8v7"/>'
       + '<path d="M18 5v13"/><path d="M22 10v3"/>',
  subtitle: '<rect width="18" height="14" x="3" y="5" rx="2" ry="2"/>'
          + '<path d="M7 15h4M15 15h2M7 11h2M13 11h4"/>',
};

const KIND_NAME = { video: "Vídeo", audio: "Audio", subtitle: "Subtítulo" };

const VERDICT = {
  copy: "copiar",
  convert: "convertir",
  extract: "extraer",
  drop: "descartar",
  unsupported: "sin soporte",
};

/** V1 / A1 / A2 / S1: identifica la pista como lo haría un editor. */
function slugger() {
  const seen = { video: 0, audio: 0, subtitle: 0 };
  const letter = { video: "V", audio: "A", subtitle: "S" };
  return (kind) => `${letter[kind] || "?"}${++seen[kind]}`;
}

export function renderTracks(list, streams) {
  const nextSlug = slugger();
  list.innerHTML = "";

  for (const s of streams) {
    const li = document.createElement("li");
    li.className = `track track--${s.action}`;

    const bits = [s.detail];
    if (s.lang && s.lang !== "und") bits.push(s.lang);
    if (s.title) bits.push(s.title);

    li.innerHTML = `
      <span class="spine"></span>
      <span class="tkind"><svg viewBox="0 0 24 24" fill="none" stroke="currentColor"
        stroke-width="2" stroke-linecap="round" stroke-linejoin="round">${KIND_ICON[s.kind] ?? ""}</svg></span>
      <span class="slug"></span>
      <span class="codec"></span>
      <span class="detail"></span>
      <span class="verdict"></span>
      <span class="reason"></span>`;

    li.querySelector(".tkind").title = KIND_NAME[s.kind] ?? s.kind;
    li.querySelector(".slug").textContent = nextSlug(s.kind);
    li.querySelector(".codec").textContent = codecName(s.codec);
    li.querySelector(".detail").textContent = bits.join(" · ");
    li.querySelector(".verdict").textContent = VERDICT[s.action] ?? s.action;

    // El resumen ya cuenta qué pasa con cada pista; aquí solo el porqué cuando
    // no es evidente. El resto queda en el tooltip.
    const needsReason = s.action === "unsupported" || s.action === "drop";
    li.querySelector(".reason").textContent = needsReason ? s.reason : "";
    li.title = s.reason;

    list.appendChild(li);
  }
}

/** El tema se elige en la ventana principal; las demás lo heredan. */
export function applyStoredTheme() {
  const choice = localStorage.getItem("theme") ?? "auto";
  if (choice === "auto") document.documentElement.removeAttribute("data-theme");
  else document.documentElement.setAttribute("data-theme", choice);
}
