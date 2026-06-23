import { execFileSync, spawnSync } from "node:child_process";
import { mkdirSync, readFileSync, readdirSync, writeFileSync } from "node:fs";
import { basename, extname, join, resolve } from "node:path";

const repo = resolve(new URL("..", import.meta.url).pathname.replace(/^\/([A-Za-z]:)/, "$1"));
const sourceDir = join(repo, "target", "image-replace-tests", "sources");
const outDir = join(repo, "target", "image-replace-tests", "previews");
mkdirSync(outDir, { recursive: true });
const maxBlocks = Number(process.env.OCR_TRANSLATOR_TEST_MAX_BLOCKS || 30);

const chrome =
  process.env.CHROME_PATH ||
  "C:\\Program Files\\Google\\Chrome\\Application\\chrome.exe";

const inputPaths = process.argv.slice(2).length
  ? process.argv.slice(2)
  : readdirSync(sourceDir)
      .filter((name) => /\.(png|jpe?g|webp)$/i.test(name))
      .map((name) => join(sourceDir, name));

function runOcr(imagePath) {
  const output = execFileSync(
    "cargo",
    ["run", "-q", "-p", "app-windows", "--example", "ocr_probe", "--", "snippingtool", imagePath],
    { cwd: repo, encoding: "utf8", stdio: ["ignore", "pipe", "pipe"] },
  );
  const [textPart, boxPart = ""] = output.split(/\r?\n-- boxes --\r?\n/);
  const blocks = [];
  for (const line of boxPart.split(/\r?\n/)) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const match = trimmed.match(/^(.*?)\t\[(.*)\]$/);
    if (!match) continue;
    blocks.push({
      text: match[1].trim(),
      bbox: match[2].split(",").map((value) => Number(value.trim())),
    });
  }
  return { text: textPart.trim(), lines: blocks };
}

function rectFromBbox(bbox) {
  const xs = [bbox[0], bbox[2], bbox[4], bbox[6]];
  const ys = [bbox[1], bbox[3], bbox[5], bbox[7]];
  const x1 = Math.max(0, Math.min(...xs));
  const y1 = Math.max(0, Math.min(...ys));
  const x2 = Math.max(...xs);
  const y2 = Math.max(...ys);
  return { x: x1, y: y1, width: Math.max(1, x2 - x1), height: Math.max(1, y2 - y1) };
}

function groupLines(lines) {
  return lines
    .filter((line) => line.text)
    .map((line) => ({ text: line.text, rect: rectFromBbox(line.bbox) }))
    .sort((a, b) => a.rect.y - b.rect.y || a.rect.x - b.rect.x)
    .map((line) => {
    const x1 = Math.max(0, line.rect.x - 3);
    const y1 = Math.max(0, line.rect.y - 2);
    const x2 = line.rect.x + line.rect.width + 3;
    const y2 = line.rect.y + line.rect.height + 2;
    return {
      source_text: line.text,
      x: x1,
      y: y1,
      width: Math.max(1, x2 - x1),
      height: Math.max(1, y2 - y1),
      line_count: 1,
    };
  });
}

async function translate(text) {
  const providers = (process.env.OCR_TRANSLATOR_TEST_PROVIDER || "bing,microsoft")
    .split(",")
    .map((value) => value.trim())
    .filter(Boolean);
  let lastError = "";
  for (const provider of providers) {
    for (let attempt = 0; attempt < 3; attempt++) {
      const result = spawnSync(
        "cargo",
        [
          "run",
          "-q",
          "-p",
          "app-core",
          "--example",
          "translate_probe",
          "--",
          provider,
          "auto",
          "zh-CN",
          text,
        ],
        { cwd: repo, encoding: "utf8" },
      );
      if (result.status === 0 && result.stdout.trim()) {
        return result.stdout.trim();
      }
      lastError = result.stderr || result.stdout || `${provider} translation failed`;
      await new Promise((resolve) => setTimeout(resolve, 600 * (attempt + 1)));
    }
  }
  throw new Error(lastError);
}

function htmlEscape(value) {
  return String(value)
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;");
}

function makePreviewHtml(imagePath, blocks) {
  const bytes = readFileSync(imagePath);
  const mime = extname(imagePath).toLowerCase() === ".jpg" || extname(imagePath).toLowerCase() === ".jpeg"
    ? "image/jpeg"
    : "image/png";
  const dataUrl = `data:${mime};base64,${bytes.toString("base64")}`;
  return `<!doctype html>
<html lang="zh-CN">
<head>
<meta charset="utf-8" />
<style>
html,body{margin:0;background:#f1f3f5;font-family:"Microsoft YaHei UI","Segoe UI",Arial,sans-serif}
.stage{position:relative;display:inline-block;background:white}
.source{display:block;max-width:none}
.layer{position:absolute;inset:0;pointer-events:none}
.block{position:absolute;display:flex;align-items:flex-start;justify-content:flex-start;overflow:hidden;padding:0 2px;white-space:pre-wrap;overflow-wrap:anywhere;word-break:break-word;font-weight:500;line-height:1.26;text-shadow:none}
.block.center{align-items:center;justify-content:center;text-align:center}
</style>
</head>
<body>
<div class="stage">
<img class="source" id="source" src="${dataUrl}" />
<div class="layer" id="layer"></div>
</div>
<script>
const blocks=${JSON.stringify(blocks)};
function median(values){values.sort((a,b)=>a-b);return values[Math.floor(values.length/2)]||0}
function luma(c){return .299*c[0]+.587*c[1]+.114*c[2]}
function hex(c){return '#'+c.map(v=>Math.max(0,Math.min(255,Math.round(v))).toString(16).padStart(2,'0')).join('')}
function fitFont(text,w,h,base){for(let f=Math.max(9,base);f>=9;f--){const chars=Math.max(1,Math.floor(w/(f*.92)));const rows=Math.max(1,text.split('\\n').reduce((s,line)=>s+Math.ceil(Math.max(1,[...line].length)/chars),0));if(rows*f*1.28<=Math.max(h,f))return f}return 9}
source.onload=()=>{
  const canvas=document.createElement('canvas');canvas.width=source.naturalWidth;canvas.height=source.naturalHeight;
  const ctx=canvas.getContext('2d',{willReadFrequently:true});ctx.drawImage(source,0,0);
  for(const b of blocks){
    const x=Math.max(0,Math.floor(b.x)),y=Math.max(0,Math.floor(b.y)),w=Math.max(1,Math.min(source.naturalWidth-x,Math.ceil(b.width))),h=Math.max(1,Math.min(source.naturalHeight-y,Math.ceil(b.height)));
    const data=ctx.getImageData(x,y,w,h).data;const px=[];for(let i=0;i<data.length;i+=4){if(data[i+3]>16)px.push([data[i],data[i+1],data[i+2]])}
    const bg=[median(px.map(p=>p[0])),median(px.map(p=>p[1])),median(px.map(p=>p[2]))];const bgL=luma(bg);
    let fg=px.filter(p=>Math.abs(luma(p)-bgL)>42 && (bgL<128?luma(p)>bgL:luma(p)<bgL));
    if(fg.length<6)fg=px.filter(p=>bgL<128?luma(p)>120:luma(p)<150);
    const tc=fg.length?[median(fg.map(p=>p[0])),median(fg.map(p=>p[1])),median(fg.map(p=>p[2]))]:(bgL<128?[242,238,228]:[30,34,42]);
    const div=document.createElement('div');div.className='block'+(b.y<source.naturalHeight*.18&&Math.abs((b.x+b.width/2)-source.naturalWidth/2)<source.naturalWidth*.18?' center':'');
    div.style.left=b.x+'px';div.style.top=b.y+'px';div.style.width=b.width+'px';div.style.height=b.height+'px';div.style.background=hex(bg);div.style.color=hex(tc);div.style.fontSize=fitFont(b.translated_text,b.width,b.height,Math.round(b.height/Math.max(1,b.line_count)*.78))+'px';
    div.innerHTML='<span>${""}</span>';div.firstChild.textContent=b.translated_text;layer.appendChild(div);
  }
}
</script>
</body>
</html>`;
}

const report = [];
for (let i = 0; i < inputPaths.length; i++) {
  const imagePath = resolve(inputPaths[i]);
  const id = `${String(i + 1).padStart(2, "0")}_${basename(imagePath).replace(/\W+/g, "_")}`;
  console.log(`[${i + 1}/${inputPaths.length}] OCR ${imagePath}`);
  try {
    const ocr = runOcr(imagePath);
    const groups = groupLines(ocr.lines).slice(0, maxBlocks);
    const translatedGroups = [];
    for (const group of groups) {
      const translated = await translate(group.source_text);
      translatedGroups.push({ ...group, translated_text: translated });
    }
    const htmlPath = join(outDir, `${id}.html`);
    const pngPath = join(outDir, `${id}.png`);
    writeFileSync(htmlPath, makePreviewHtml(imagePath, translatedGroups), "utf8");
    const shot = spawnSync(
      chrome,
      [
        "--headless=new",
        "--disable-gpu",
        "--hide-scrollbars",
        "--window-size=1400,1000",
        `--screenshot=${pngPath}`,
        `file:///${htmlPath.replaceAll("\\", "/")}`,
      ],
      { encoding: "utf8" },
    );
    if (shot.status !== 0) throw new Error(shot.stderr || shot.stdout);
    report.push({
      image: imagePath,
      text_length: ocr.text.length,
      ocr_lines: ocr.lines.length,
      replacement_blocks: translatedGroups.length,
      preview: pngPath,
      ok: ocr.lines.length > 0 && translatedGroups.length > 0,
    });
  } catch (error) {
    report.push({
      image: imagePath,
      error: String(error?.message || error),
      ok: false,
    });
  }
}

const reportPath = join(outDir, "report.json");
writeFileSync(reportPath, JSON.stringify(report, null, 2), "utf8");
const passed = report.filter((item) => item.ok).length;
console.log(`image replace smoke: ${passed}/${report.length} passed`);
console.log(reportPath);
