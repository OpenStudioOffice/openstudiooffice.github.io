(() => {
  const $ = id => document.getElementById(id);
  const input = $("file-input"), zone = $("drop-zone"), browse = $("browse-button");
  const panel = $("file-panel"), convert = $("convert-button"), format = $("output-format");
  const bitrate = $("bitrate"), bitrateWrap = $("bitrate-wrap"), status = $("status-message");
  const progressWrap = $("progress-wrap"), progressBar = $("progress-bar"), progressText = $("progress-text"), progressPercent = $("progress-percent");
  const result = $("result-panel"), download = $("download-link");
  let selectedFile = null, decodedBuffer = null, outputUrl = null;

  const bytes = n => n < 1024 ? `${n} B` : n < 1048576 ? `${(n/1024).toFixed(1)} KB` : `${(n/1048576).toFixed(2)} MB`;
  const setProgress = (p, label) => { progressWrap.classList.remove("hidden"); progressBar.style.width = `${p}%`; progressPercent.textContent = `${p}%`; progressText.textContent = label; };
  function clearResult() { result.classList.add("hidden"); if (outputUrl) URL.revokeObjectURL(outputUrl); outputUrl = null; }
  function chooseFile(file) {
    if (!file) return;
    if (file.size > 300 * 1024 * 1024) { status.textContent = "This demo supports files up to 300 MB. Try a smaller file."; return; }
    selectedFile = file; decodedBuffer = null; clearResult(); status.textContent = "";
    $("file-name").textContent = file.name; $("file-meta").textContent = `${bytes(file.size)} · Selected`;
    panel.classList.remove("hidden"); zone.classList.add("has-file"); convert.disabled = false;
    progressWrap.classList.add("hidden");
  }
  zone.addEventListener("click", e => { if (e.target !== browse) input.click(); });
  zone.addEventListener("keydown", e => { if (e.key === "Enter" || e.key === " ") { e.preventDefault(); input.click(); } });
  browse.addEventListener("click", e => { e.stopPropagation(); input.click(); });
  input.addEventListener("change", () => chooseFile(input.files[0]));
  $("remove-file").addEventListener("click", () => { selectedFile = null; decodedBuffer = null; input.value = ""; panel.classList.add("hidden"); convert.disabled = true; clearResult(); status.textContent = ""; progressWrap.classList.add("hidden"); });
  ["dragenter","dragover"].forEach(ev => zone.addEventListener(ev, e => { e.preventDefault(); zone.classList.add("dragging"); }));
  ["dragleave","drop"].forEach(ev => zone.addEventListener(ev, e => { e.preventDefault(); zone.classList.remove("dragging"); }));
  zone.addEventListener("drop", e => chooseFile(e.dataTransfer.files[0]));
  format.addEventListener("change", () => bitrateWrap.classList.toggle("hidden", format.value !== "mp3"));
  // Match the default output to the intent of the dedicated landing page.
  if (window.location.pathname.replace(/\/+$/, "").endsWith("/mp3-to-wav")) {
    format.value = "wav";
    bitrateWrap.classList.add("hidden");
  }
  document.querySelectorAll("[data-format]").forEach(a => a.addEventListener("click", () => { format.value = a.dataset.format; bitrateWrap.classList.toggle("hidden", format.value !== "mp3"); }));
  $("year").textContent = new Date().getFullYear();

  async function decode(file) {
    const context = new (window.AudioContext || window.webkitAudioContext)();
    try { const buffer = await file.arrayBuffer(); return await context.decodeAudioData(buffer); }
    finally { await context.close(); }
  }
  function resampleChannel(channel, fromRate, toRate) {
    if (fromRate === toRate) return channel;
    const ratio = fromRate / toRate, length = Math.round(channel.length / ratio), out = new Float32Array(length);
    for (let i=0; i<length; i++) {
      const pos = i * ratio, left = Math.floor(pos), right = Math.min(left+1, channel.length-1), frac = pos-left;
      out[i] = channel[left] * (1-frac) + channel[right] * frac;
    }
    return out;
  }
  function toWav(buffer) {
    const channels = Math.min(buffer.numberOfChannels, 2), sampleRate = buffer.sampleRate;
    const channelData = Array.from({length:channels}, (_,i)=>buffer.getChannelData(i));
    const frames = buffer.length, bytesPerSample = 2, out = new ArrayBuffer(44 + frames * channels * bytesPerSample), view = new DataView(out);
    const write = (offset, text) => { for(let i=0;i<text.length;i++) view.setUint8(offset+i,text.charCodeAt(i)); };
    write(0,"RIFF"); view.setUint32(4,36+frames*channels*2,true); write(8,"WAVE"); write(12,"fmt "); view.setUint32(16,16,true); view.setUint16(20,1,true); view.setUint16(22,channels,true); view.setUint32(24,sampleRate,true); view.setUint32(28,sampleRate*channels*2,true); view.setUint16(32,channels*2,true); view.setUint16(34,16,true); write(36,"data"); view.setUint32(40,frames*channels*2,true);
    let offset=44;
    for(let i=0;i<frames;i++) for(let ch=0;ch<channels;ch++){ let s=Math.max(-1,Math.min(1,channelData[ch][i])); view.setInt16(offset,s<0?s*0x8000:s*0x7fff,true); offset+=2; }
    return new Blob([out],{type:"audio/wav"});
  }
  async function toMp3(buffer, kbps) {
    if (!window.lamejs || !window.lamejs.Mp3Encoder) throw new Error("The MP3 encoder did not load. Check your internet connection, then reload the page.");
    const sampleRate = 44100, channels = Math.min(buffer.numberOfChannels,2);
    const data = Array.from({length:channels},(_,i)=>resampleChannel(buffer.getChannelData(i),buffer.sampleRate,sampleRate));
    const encoder = new window.lamejs.Mp3Encoder(channels,sampleRate,Number(kbps));
    const blockSize = 1152, chunks = [], total = data[0].length;
    for(let pos=0;pos<total;pos+=blockSize){
      const left = new Int16Array(Math.min(blockSize,total-pos)), right = channels===2 ? new Int16Array(left.length) : null;
      for(let j=0;j<left.length;j++){ const a=data[0][pos+j]||0; left[j]=Math.max(-32768,Math.min(32767,Math.round(a<0?a*32768:a*32767))); if(right){const b=data[1][pos+j]||0;right[j]=Math.max(-32768,Math.min(32767,Math.round(b<0?b*32768:b*32767)));} }
      const encoded = channels===2 ? encoder.encodeBuffer(left,right) : encoder.encodeBuffer(left);
      if(encoded.length) chunks.push(new Int8Array(encoded));
      if(pos % (blockSize*50)===0){setProgress(Math.min(90,Math.round(pos/total*90)),"Encoding MP3…"); await new Promise(r=>setTimeout(r,0));}
    }
    const end=encoder.flush(); if(end.length) chunks.push(new Int8Array(end));
    return new Blob(chunks,{type:"audio/mpeg"});
  }
  convert.addEventListener("click", async () => {
    if(!selectedFile) return;
    convert.disabled=true; clearResult(); status.textContent=""; setProgress(2,"Reading audio file…");
    try {
      decodedBuffer = await decode(selectedFile);
      setProgress(10,"Audio decoded. Preparing conversion…");
      const blob = format.value==="wav" ? (setProgress(30,"Creating WAV file…"), toWav(decodedBuffer)) : await toMp3(decodedBuffer,bitrate.value);
      setProgress(100,"Conversion complete");
      const base=selectedFile.name.replace(/\.[^.]+$/,"").replace(/[^\w\- ()]+/g,"_") || "audio";
      const ext=format.value==="wav"?"wav":"mp3"; outputUrl=URL.createObjectURL(blob);
      download.href=outputUrl; download.download=`${base}.${ext}`;
      $("result-name").textContent=`${base}.${ext}`; $("result-meta").textContent=`${bytes(blob.size)} · ${ext.toUpperCase()} audio`;
      result.classList.remove("hidden"); status.textContent="Your converted file is ready to download.";
    } catch(err) {
      console.error(err); status.textContent = `Could not convert this file. ${err.message || "Try a different audio format or browser."}`;
      progressWrap.classList.add("hidden");
    } finally { convert.disabled=false; }
  });
})();
  document.querySelectorAll(".waveform").forEach(el => {
    const heights = [22,35,48,29,62,78,42,31,56,86,65,34,47,72,93,58,38,24,53,81,67,44,29,60,90,71,42,25,49,77,55,36,68,95,61,32,45,73,51,28,64,83,40,22,57,76,46,31,69,88,50,26,43,72,58,35,80,61,30,47,91,65,38,55];
    el.innerHTML = heights.map(h => `<i style="height:${h}%"></i>`).join("");
  });
