'use strict';
// This is a local interaction prototype. No native capture, network, microphone or file mutation.
const $ = (s, p=document) => p.querySelector(s);
const $$ = (s,p=document) => [...p.querySelectorAll(s)];
const esc = s => String(s).replace(/[&<>"']/g,c=>({'&':'&amp;','<':'&lt;','>':'&gt;','"':'&quot;',"'":'&#39;'}[c]));
const paths = {
 minus:'M5 12h14', window:'M5 5h14v14H5z', close:'m6 6 12 12M18 6 6 18',
 sun:'M12 3V1m0 22v-2M3 12H1m22 0h-2M4 4l2 2m12 12 2 2M4 20l2-2M18 6l2-2M17 12a5 5 0 1 1-10 0 5 5 0 0 1 10 0',
 moon:'M20 15A9 9 0 0 1 9 4a9 9 0 1 0 11 11', menu:'M4 6h16M4 12h16M4 18h16',
 panel:'M4 4h16v16H4zM14 4v16', left:'m14 6-6 6 6 6', right:'m9 6 6 6-6 6',
 folder:'M3 6h7l2 2h9v11H3z', open:'M3 7h7l2 2h8l-3 10H2l1-12',
 image:'M4 4h16v16H4zM4 16l5-6 5 6 3-3 3 4M15 8h.01',
 video:'M3 6h13v12H3zM16 10l5-3v10l-5-3', mic:'M9 4a3 3 0 0 1 6 0v8a3 3 0 0 1-6 0zM6 10v2a6 6 0 0 0 12 0v-2M12 18v4M8 22h8',
 subtitle:'M3 5h18v14H3zM6 10h4m3 0h5M6 14h6m3 0h3', music:'M9 17V5l11-2v12M9 8l11-2M9 17c0 4-6 5-6 1s6-5 6-1m11-2c0 4-6 5-6 1s6-5 6-1',
 capture:'M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5M7 7h10v10H7z',
 record:'M19 12a7 7 0 1 1-14 0 7 7 0 0 1 14 0',
 select:'m5 3 14 10-7 1-3 7z', crop:'M7 2v15h15M2 7h15v15M7 7h10',
 pen:'m4 16 11-11 4 4L8 20H4zM13 7l4 4', rectangle:'M4 5h16v14H4z',
 arrow:'M4 19 19 4M7 4h12v12', text:'M4 4h16M12 4v16M8 20h8',
 blur:'M4 4h7v7H4zm9 0h7v7h-7zM4 13h7v7H4zm9 0h7v7h-7z',
 pin:'m14 3 7 7-4 1-5 5-4-4 5-5zM8 16l-5 5',
 ocr:'M6 3H3v4m15-4h3v4M3 17v4h3m15-4v4h-3M7 8h10M12 8v9M9 17h6',
 spark:'m12 2 2.5 7.5L22 12l-7.5 2.5L12 22l-2.5-7.5L2 12l7.5-2.5z',
 collage:'M3 3h8v8H3zm10 0h8v8h-8zM3 13h8v8H3zm10 0h8v8h-8z',
 plus:'M12 4v16M4 12h16', undo:'M8 4 3 9l5 5M3 9h10a6 6 0 0 1 0 12',
 redo:'m16 4 5 5-5 5m5-5H11a6 6 0 0 0 0 12',
 trash:'M4 6h16M9 6V3h6v3M6 6l1 15h10l1-15M10 10v7m4-7v7',
 cut:'M9 7a3 3 0 1 1-6 0 3 3 0 0 1 6 0m0 10a3 3 0 1 1-6 0 3 3 0 0 1 6 0M8 9 20 21M8 15 20 3',
 play:'m8 4 12 8-12 8z', pause:'M8 5v14M16 5v14', stop:'M5 5h14v14H5z',
 first:'M5 5v14m13-14-9 7 9 7z', last:'M19 5v14M6 5l9 7-9 7z',
 sound:'M3 9h4l5-5v16l-5-5H3zM16 8q5 4 0 8m2-11q8 7 0 14',
 mute:'M3 9h4l5-5v16l-5-5H3zM16 9l5 6m0-6-5 6',
 fit:'M8 3H3v5m13-5h5v5M3 16v5h5m13-5v5h-5',
 zoom:'M16 10a6 6 0 1 1-12 0 6 6 0 0 1 12 0m-1 5 6 6M7 10h6M10 7v6',
 zoomout:'M16 10a6 6 0 1 1-12 0 6 6 0 0 1 12 0m-1 5 6 6M7 10h6',
 search:'M16 10a6 6 0 1 1-12 0 6 6 0 0 1 12 0m-1 5 6 6',
 grid:'M4 4h6v6H4zm10 0h6v6h-6zM4 14h6v6H4zm10 0h6v6h-6z',
 list:'M8 5h13M8 12h13M8 19h13M3 5h.01M3 12h.01M3 19h.01',
 down:'m6 9 6 6 6-6', check:'m5 12 4 4L20 5', eye:'M2 12s4-7 10-7 10 7 10 7-4 7-10 7S2 12 2 12m13 0a3 3 0 1 1-6 0 3 3 0 0 1 6 0',
 refresh:'M20 10a8 8 0 1 0 0 5M20 3v7h-7', settings:'M4 7h16M4 17h16M9 4v6m6 4v6',
 save:'M4 3h13l4 4v14H3V3zM7 3v6h9V3M7 21v-8h10v8',
 copy:'M8 8h12v13H8zM15 8V3H3v13h5',
 download:'M12 3v12m-5-5 5 5 5-5M4 17v4h16v-4',
 upload:'M12 16V4m-5 5 5-5 5 5M4 17v4h16v-4',
 link:'m9 15 6-6M8 18l-2 2a4 4 0 0 1-6-6l5-5a4 4 0 0 1 6 0m2-3 2-2a4 4 0 0 1 6 6l-5 5a4 4 0 0 1-6 0',
 plugin:'M8 3v4m8-4v4M5 7h14v5a7 7 0 0 1-14 0zM12 19v3',
 clock:'M21 12a9 9 0 1 1-18 0 9 9 0 0 1 18 0M12 6v6l4 3',
 lock:'M6 10h12v11H6zM8 10V6a4 4 0 0 1 8 0v4', more:'M5 12h.01M12 12h.01M19 12h.01'
};
const ico = name => `<svg class="icon" viewBox="0 0 24 24" aria-hidden="true"><path d="${paths[name]||paths.more}"/></svg>`;
const ib=(icon,label,action,extra='')=>`<button class="icon-button" aria-label="${esc(label)}" data-tip="${esc(label)}" data-action="${action}" ${extra}>${ico(icon)}</button>`;
const btn=(label,action,cls='secondary',extra='')=>`<button class="${cls}" data-action="${action}" ${extra}>${label}</button>`;
const sep='<span class="sep"></span>';
const section=(title,body)=>`<section class="inspector-section">${title?`<h3 class="section-heading">${title}</h3>`:''}${body}</section>`;
const num=(label,value,unit='',id='',min=0,max=10000,step=1)=>`<label class="field"><span>${label}</span><span class="unit"><input type="number" aria-label="${esc(label.replace(/<[^>]*>/g,''))}" value="${value}" min="${min}" max="${max}" step="${step}" ${id?`data-value="${id}"`:''}>${unit}</span></label>`;
const choice=(label,items,id)=>`<label class="field"><span>${label}</span><select aria-label="${label}" data-value="${id}">${items.map(t=>`<option>${t}</option>`).join('')}</select></label>`;
const segments=(items,current,action)=>`<div class="segmented">${items.map(([id,label])=>btn(label,action+':'+id,current===id?'selected':'','aria-pressed="'+(current===id)+'"')).join('')}</div>`;
const wave=()=>`<div class="waveform">${Array.from({length:120},(_,i)=>`<i style="height:${3+Math.abs(Math.sin(i*1.37)+Math.sin(i*.31))*7}px"></i>`).join('')}</div>`;
const sceneries=[['#dce3e5','#9caeb4','#697c83','#4b5f66'],['#e5ddd0','#c3ab92','#9b8873','#706655'],['#dae1d9','#9eafa0','#778d7d','#506858'],['#dedee8','#b1adbf','#89879e','#616276']];
function scene(n=0,label=false){
 const c=sceneries[n%4];
 return `<div class="scene"><svg viewBox="0 0 800 450" preserveAspectRatio="xMidYMid slice" aria-hidden="true"><rect width="800" height="450" fill="${c[0]}"/><circle cx="614" cy="106" r="53" fill="#f6f3e9"/><path d="M0 355 234 166 420 348 590 221 800 368V450H0Z" fill="${c[1]}"/><path d="m0 400 191-101 216 59L641 178l159 129v143H0Z" fill="${c[2]}"/><path d="M0 450 224 379 489 412 672 334l128 46v70Z" fill="${c[3]}"/><path d="M318 450 405 362 482 350 429 400l-22 50Z" fill="${c[0]}" opacity=".45"/></svg>${label?'<div class="scene-kicker">FIELD NOTES / 01</div><div class="scene-name">把灵感，留在画面里。</div><div class="scene-caption">捕捉 · 整理 · 表达</div>':''}</div>`;
}
const assets=[
 {name:'山间手记 · 演示片.mp4',type:'video',meta:'00:24 · 1920 × 1080',scene:0},
 {name:'产品说明.png',type:'image',meta:'1600 × 1067 · 1.2 MB',scene:1},
 {name:'旁白 · 第一版.wav',type:'audio',meta:'00:18 · 48 kHz',scene:3},
 {name:'山间手记.panzo',type:'project',meta:'项目 · 今天 14:32',scene:0},
 {name:'灵感截图 02.png',type:'image',meta:'1920 × 1080 · 842 KB',scene:2},
 {name:'剪辑封面.png',type:'image',meta:'1600 × 900 · 621 KB',scene:3},
 {name:'片尾画面.png',type:'image',meta:'1920 × 1080 · 780 KB',scene:1},
 {name:'配乐 · 晨光.mp3',type:'audio',meta:'01:32 · 44.1 kHz',scene:2},
 {name:'录屏片段 02.mp4',type:'video',meta:'00:12 · 1920 × 1080',scene:1}
];
const S=window.S={
 page:new URLSearchParams(location.search).get('page')||'video', inspector:true, bin:true, videoTab:'clip', selectedClip:'v1',
 imageTool:'select', hasImage:true, modified:false, zoom:1, pan:{x:0,y:0}, play:false, time:5.3, speed:1,
 plugin:true, recording:false, recordSeconds:0, task:null, selectedSubtitle:0, subtitles:['把灵感，留在画面里。','从一次捕捉，到完整表达。','让每个想法，都有自己的形状。'],
 clips:[{id:'v1',kind:'video',start:0,end:9.5,name:'屏幕录制 01'},{id:'v2',kind:'video',start:9.5,end:24,name:'屏幕录制 02'},
 {id:'a1',kind:'audio',start:1.3,end:19,name:'旁白 · 第一版'},{id:'m1',kind:'music',start:0,end:24,name:'晨光.mp3'},
 {id:'s1',kind:'subtitle',start:2,end:7.2,name:'把灵感，留在画面里。'},{id:'s2',kind:'subtitle',start:8.2,end:14.3,name:'从一次捕捉，到完整表达。'},{id:'s3',kind:'subtitle',start:15.2,end:22,name:'让每个想法，都有自己的形状。'}],
 undo:[], ratio:'free', crop:{x:7,y:8,w:86,h:82}, collageCount:4, collageRatio:'1:1', collageDirection:'horizontal', fit:'cover', gap:12,bg:'#f6f4ef',cell:0,
 cells:Array.from({length:9},(_,i)=>({scene:i%4,z:1,x:0,y:0})), result:'', query:'', filter:'all', asset:0, assetView:'grid', settingsTab:'general', timelineHeight:null, assetPreview:false
};
S.dirty={image:false,video:false,assets:false};S.histories={image:[],video:[],assets:[]};
Object.defineProperty(S,'modified',{get(){return S.dirty[S.page]||false},set(value){S.dirty[S.page]=value}});
Object.defineProperty(S,'undo',{get(){return S.histories[S.page]}});
try{document.documentElement.dataset.theme=localStorage.getItem('panzo-v14-theme')||'dark';}catch{}
function savePoint(){S.undo.push(JSON.stringify(S.page==='image'?{imageTool:S.imageTool,hasImage:S.hasImage,annotated:S.annotated,cropped:S.cropped,rotated:S.rotated}:{clips:S.clips,subtitles:S.subtitles,speed:S.speed}));S.modified=true;}
function toast(text){$('#toast').textContent=text;$('#toast').hidden=false;clearTimeout(toast.timer);toast.timer=setTimeout(()=>$('#toast').hidden=true,2400);}
function theme(){const root=document.documentElement;root.dataset.theme=root.dataset.theme==='dark'?'light':'dark';try{localStorage.setItem('panzo-v14-theme',root.dataset.theme);}catch{}updateTheme();}
function updateTheme(){const dark=document.documentElement.dataset.theme==='dark';$('#theme').innerHTML=ico(dark?'sun':'moon');$('#theme').setAttribute('aria-label',dark?'切换浅色主题':'切换深色主题');}
function render(){
 $('#app').dataset.page=S.page;
 $$('.workspaces button').forEach(b=>{if(b.dataset.page===S.page)b.setAttribute('aria-current','page');else b.removeAttribute('aria-current');});
 $('.export-button').hidden=S.page==='assets';
 renderContext();
 $('#main').innerHTML=S.page==='video'?video():S.page==='image'?imageWorkspace():library();
 if(S.timelineHeight){const t=$('.timeline');if(t){t.style.height=S.timelineHeight+'px';t.classList.toggle('compact',S.timelineHeight<200);}}
 bindStage();updatePreview();updateTask();updateTheme();
 $$('[data-icon]').forEach(b=>b.innerHTML=ico(b.dataset.icon));
}
function renderContext(){
 let content='';
 if(S.page==='video'){
  content=ib('panel','显示 / 收起项目素材','bin')+`<span class="saved">${S.modified?'未保存':'已保存'}</span>`+sep+
   ib('record','区域录屏','record')+ib('folder','导入素材','import')+`<div class="spacer"></div><span class="zoom-label">${Math.round(S.zoom*100)}%</span>`+ib('fit','预览居中并适应窗口','fit');
 }else if(S.page==='image'){
  content=ib('menu','工作区菜单','menu-image')+`<span class="saved">${S.modified?'未保存':'已保存'}</span>`+
  (S.hasImage?`<button class="doc-tab" data-action="image-info">${ico('image')}<span class="truncate">产品说明.png</span></button>`:'<span class="document-title">图片工具</span>')+
  ib('capture','区域截图 · Ctrl+Alt+Shift+F9','capture')+ib('folder','打开图片','open-image')+sep+ib('undo','撤销 · Ctrl+Z','undo','class="extra"')+
  `<div class="spacer"></div><span class="zoom-label">${Math.round(S.zoom*100)}%</span>`+ib('fit','居中并适应窗口','fit');
 }else{
  content=ib('menu','显示 / 收起分类','bin')+'<span class="saved">本地</span><span class="document-title">素材库</span>'+sep+ib('folder','选择素材目录','directory')+`<span class="path-line truncate" data-action="edit-path" tabindex="0">D:\\素材\\Panzo</span><div class="spacer"></div>`+ib('refresh','刷新素材库','refresh-assets');
 }
 $('#contextbar').innerHTML=`<div class="context-main">${content}${!S.inspector?ib('panel','展开属性面板','inspector'):''}</div>${S.inspector?`<div class="context-title"><span>${S.page==='assets'?'预览与信息':S.page==='image'?'工具与属性':'项目与片段'}</span><div class="spacer"></div>${ib('panel','收起属性面板','inspector')}</div>`:''}`;
}
function projectBin(){
 if(!S.bin&&innerWidth>1000)return '';
 return `<aside class="project-bin ${S.compactBin?'compact-open':''}"><div class="bin-head"><span>项目素材 <span class="muted small">4</span></span>${ib('more','素材管理','bin-menu')}</div><div class="bin-actions">${ib('plus','导入素材','import')}${ib('record','区域录屏','record')}${ib('folder','从素材库添加','open-library')}</div><div class="bin-search">${ico('search')}<input aria-label="搜索项目素材" placeholder="搜索当前项目" data-value="bin-search"></div>${[assets[0],{...assets[1],name:'录屏片段 02.mp4',meta:'00:14 · 视频'},assets[2],assets[7]].map((a,i)=>`<div class="bin-item ${i===0?'selected':''}" tabindex="0" data-bin="${i}"><div class="bin-thumb">${a.type==='audio'?wave():scene(a.scene)}</div><div class="bin-copy"><div class="truncate">${a.name}</div><small>${a.meta}</small></div></div>`).join('')}<div class="bin-foot">当前项目 · 4 个文件</div></aside>`;
}
function inspector(tabs,body,footer='',cls=''){
 if(!S.inspector)return '';
 return `<div class="splitter-vertical" role="separator" aria-label="调整属性面板宽度" aria-orientation="vertical" tabindex="0"></div><aside class="inspector ${cls}">${tabs?`<div class="inspector-tabs">${tabs}</div>`:''}<div class="inspector-body">${body}</div>${footer?`<div class="inspector-footer">${footer}</div>`:''}</aside>`;
}
function video(){
 const tabs=[['canvas','画面'],['clip','片段'],['audio','声音'],['subtitle','字幕']].map(([v,l])=>btn(l,'video-tab:'+v,S.videoTab===v?'selected':'','aria-pressed="'+(S.videoTab===v)+'"')).join('');
 let body='',footer='';
 const selected=S.clips.find(c=>c.id===S.selectedClip);
 if(S.videoTab==='canvas'){
  body=section('项目画面',segments([['16:9','16:9'],['4:3','4:3'],['1:1','1:1'],['9:16','9:16']],S.canvasRatio||'16:9','canvas-ratio')+
    choice('分辨率',['1920 × 1080','2560 × 1440','3840 × 2160'],'resolution'))+
    section('背景',`<div class="field"><span>背景颜色</span><input type="color" aria-label="背景颜色" value="#1e2023" data-value="canvas-color"></div>`+num('边距',24,'px','padding',0,120))+
    `<details class="details"><summary>圆角与阴影</summary>${num('圆角',8,'px','rounding',0,64)}${num('阴影',12,'%','shadow',0,100)}</details>`;
 }else if(S.videoTab==='clip'){
  if(!selected)body=section('选择片段','<div class="feature-off">在轨道中选择视频、旁白或字幕，调整对应属性。</div>');
  else{
   const a=selected.kind==='audio'||selected.kind==='music';
   body=section('',`<div class="selection-heading">${ico(a?'sound':'video')}${esc(selected.name)}</div><div class="selection-meta">${a?'音频片段':'视频片段'} · ${(selected.end-selected.start).toFixed(2)} 秒</div>`)+
    section('时间范围',`<div class="two-fields">${num('入点',selected.start.toFixed(2),'s','clip-start',0,24,.1)}${num('出点',selected.end.toFixed(2),'s','clip-end',0,24,.1)}</div>`)+
    (a?section('声音',num('音量',100,'%','volume',0,200)+`<div class="slider-field"><input aria-label="调整音量" type="range" min="0" max="200" value="100" data-value="volume"></div>`+
      `<div class="two-fields">${num('淡入',.5,'s','fadein',0,5,.1)}${num('淡出',1,'s','fadeout',0,5,.1)}</div>`):
     section('',`<div class="field"><span>播放速度</span><span class="unit"><input aria-label="播放速度" type="number" min=".25" max="4" step=".25" value="${S.speed}" data-value="speed">×</span></div><div class="slider-field"><input aria-label="速度滑块" type="range" min=".25" max="4" step=".25" value="${S.speed}" data-value="speed"></div><div class="chips">${[.5,1,1.5,2].map(v=>btn(v+'×','speed:'+v,v===S.speed?'selected':'')).join('')}</div>`)+
     section('画面裁剪',segments([['free','自由'],['16:9','16:9'],['4:3','4:3'],['1:1','1:1']],S.ratio,'ratio')+`<div class="row" style="margin-top:10px">${btn('在预览中裁剪','video-crop','secondary full')}${ib('refresh','恢复原始画面','reset-crop')}</div>`)+
     `<details class="details"><summary>鼠标跟踪与缩放</summary><div class="field"><span>鼠标跟踪</span><button class="toggle ${S.follow?'on':''}" data-action="follow" aria-label="鼠标跟踪" aria-pressed="${!!S.follow}"></button></div>${num('放大倍数',1.5,'×','followzoom',1,3,.1)}</details>`);
  }
 }else if(S.videoTab==='audio'){
  if(!S.plugin)body=disabledPlugin();
  else{
   body=section('',`<div class="row">${btn('导入音频','import-audio','secondary full')}</div>`+choice('添加为',['旁白','背景音乐'],'audio-role'))+
    section('录制旁白',`<div class="field"><span>录音设备</span>${ib('refresh','重新检测录音设备','refresh-device')}</div><select aria-label="录音设备"><option>系统默认 · USB 麦克风</option>耳机麦克风 · Realtek Audio</option>桌面麦克风 · USB Audio</option>模拟设备断开</option></select><div class="rec-time">${clock(S.recordSeconds)}</div><div class="meter ${S.recording?'live':''}">${Array.from({length:20},()=>'<i></i>').join('')}</div><div class="info-row"><span>输入电平</span><span>−18 dB</span></div>`);
   footer=S.recording?btn('结束录制','stop-recording','primary'):btn('录制旁白','start-recording','primary');
  }
 }else{
  if(!S.plugin)body=disabledPlugin();
  else{
   body=section('字幕来源',`<div class="row">${btn('AI 自动字幕','ai-subtitle','secondary full')}${ib('folder','导入 SRT 字幕','import-subtitle')}${ib('plus','添加一条字幕','add-subtitle')}</div><div class="info-row"><span>音轨：视频原声</span>${btn('模型设置','settings:speech','')}</div>${S.task==='subtitles'?'<div class="status-inline">正在生成字幕…</div><div class="progress"><span></span></div>':''}`)+
    section('当前字幕 <span class="muted small">'+(S.selectedSubtitle+1)+' / '+S.subtitles.length+'</span>',`<textarea aria-label="当前字幕文字" data-value="subtitle-text">${esc(S.subtitles[S.selectedSubtitle]||'')}</textarea><div class="two-fields">${num('开始',S.selectedSubtitle*6+2,'s','sub-start',0,24,.1)}${num('结束',S.selectedSubtitle*6+7.2,'s','sub-end',0,24,.1)}</div>`)+
    `<details class="details"><summary>全部字幕 · ${S.subtitles.length} 条</summary><div class="subtitle-list">${S.subtitles.map((text,i)=>`<button class="subtitle-row ${S.selectedSubtitle===i?'active':''}" data-action="subtitle:${i}"><small>00:${String(i*6+2).padStart(2,'0')}.000 — 00:${String(i*6+7).padStart(2,'0')}.200</small><span>${esc(text)}</span></button>`).join('')}</div></details>`+
    `<details class="details"><summary>全局字幕样式</summary>${num('字号',32,'px','sub-size',12,80)}<div class="field"><span>文字颜色</span><input aria-label="文字颜色" type="color" value="#ffffff"></div></details>`;
   footer=btn('应用修改','apply-subtitle','primary')+ib('download','导出 SRT 字幕','export-srt');
  }
 }
 return `<section class="workbench">${projectBin()}<div class="preview-zone"><div class="canvas" id="canvas"><span class="preview-meta">1920 × 1080 · 30 fps</span><div class="stage-content"><div class="media-frame">${scene(0,true)}<div class="subtitle-preview"><span id="subtitle-preview">${esc(S.subtitles[S.selectedSubtitle]||'')}</span></div>${S.videoCropping?cropOverlay():''}</div></div></div><div class="transport"><div class="transport-center">${ib('first','跳到开头','first')}${ib(S.play?'pause':'play',S.play?'暂停 · Space':'播放 · Space','play')}${ib('last','跳到结尾','last')}<span class="timecode">${clock(S.time)} <span class="muted">/ 00:24.000</span></span></div>${ib('sound','静音预览','mute')}</div></div>${inspector(tabs,body,footer)}</section>${timeline()}`;
}
function timeline(){
 const labels={video:['video','视频'],audio:['mic','旁白'],music:['music','音乐'],subtitle:['subtitle','字幕']};
 return `<section class="timeline"><div class="timeline-divider" role="separator" aria-label="调整时间线高度" aria-orientation="horizontal" tabindex="0"></div><div class="timeline-tools">${ib('undo','撤销 · Ctrl+Z','undo',S.undo.length?'':'disabled')}${ib('redo','重做（原型未模拟）','not-modeled','disabled')}${sep}${ib('cut','在播放头处分割 · B','split')}${ib('trash','删除所选片段 · Delete','delete-clip')}${sep}${ib('plus','添加素材','import')}${ib('mic','声音与旁白','video-tab:audio')}${ib('subtitle','字幕','video-tab:subtitle')}<span class="spacer"></span><span class="small muted">时长 00:24</span>${ib('zoomout','缩小时间线','timeline-out')}<input aria-label="时间线缩放" type="range" value="30" data-value="timeline-zoom">${ib('zoom','放大时间线','timeline-in')}${ib('fit','时间线适应窗口','timeline-fit')}</div><div class="track-scroll"><div class="timeline-grid"><div class="time-ruler" aria-label="时间标尺">${Array.from({length:13},(_,i)=>`<span>00:${String(i*2).padStart(2,'0')}</span>`).join('')}</div>${Object.entries(labels).map(([kind,[icon,label]])=>`<div class="track-row ${kind==='video'?'video':''}"><div class="track-label">${ico(icon)}${label}${ib(kind==='subtitle'?'eye':'sound',label+'轨道显示 / 静音','mute-track:'+kind)}</div><div class="track-lane" data-lane="${kind}">${S.clips.filter(c=>c.kind===kind).map(c=>`<button class="clip ${kind==='audio'||kind==='music'?'audio':kind} ${S.selectedClip===c.id?'selected':''}" data-action="clip:${c.id}" style="left:${c.start/24*100}%;width:${(c.end-c.start)/24*100}%" aria-label="${esc(c.name)}"><i class="trim"></i><span class="clip-caption">${esc(c.name)}</span>${kind==='video'?`<div class="filmframes">${Array.from({length:14},()=>scene(c.id==='v1'?0:1)).join('')}</div>`:kind==='audio'||kind==='music'?wave():''}<i class="trim"></i></button>`).join('')}</div></div>`).join('')}<div class="playhead"></div></div></div></section>`;
}
function disabledPlugin(){return `<div class="feature-off">${ico('plugin')}<p>声音与 AI 字幕插件已停用。</p>${btn('打开插件管理','plugins','secondary')}</div>`;}
function paper(){return `<div class="paper-inner"><div class="paper-kicker">PANZO / FIELD NOTES</div><div class="paper-title">让灵感，<br>有迹可循。</div><div class="paper-text">记录眼前的画面<br>整理片刻的思绪<br>把想法变成完整的表达</div><div class="paper-line"></div><div class="paper-tag">捕捉 · 标注 · 创作</div><div class="paper-number">CREATIVE WORKFLOW — 2026</div><div class="paper-visual">${scene(0)}</div></div>`;}
function cropOverlay(){const c=S.crop;return `<div class="crop-box" style="left:${c.x}%;top:${c.y}%;width:${c.w}%;height:${c.h}%"><i class="horizontal"></i><i class="horizontal second"></i>${['nw','ne','sw','se'].map(k=>`<i class="crop-handle ${k}" data-corner="${k}"></i>`).join('')}<span class="crop-size">1376 × 875</span></div>`;}
function imageWorkspace(){
 const tools=[['select','选择'],['crop','裁剪'],['pen','画笔'],['rectangle','矩形'],['arrow','箭头'],['text','文字'],['blur','马赛克'],['ocr','文字 OCR'],['spark','多模态识图'],['collage','拼图'],['pin','贴图']];
 const rail=`<nav class="tool-rail" aria-label="图片工具">${tools.map(([k,label],i)=>(i===7?sep:'')+ib(k,label,'tool:'+k,`aria-pressed="${S.imageTool===k}"`)).join('')}</nav>`;
 let body='',footer='',visual='',bottom='',tool=S.imageTool;
 if(tool==='collage'){
  body=section('排版布局',`<div class="shape-buttons">${[[2,'两图'],[3,'三图'],[4,'四宫格'],[9,'九宫格']].map(([n,l])=>btn(`<span class="mini-layout ${n===3||n===9?'three':''}">${Array.from({length:n},()=>'<i></i>').join('')}</span>${l}`,'collage:'+n,S.collageCount===n?'selected':'')).join('')}</div>`)+
   section('排列与显示',segments([['horizontal','横向'],['vertical','纵向']],S.collageDirection,'direction')+`<div style="height:12px"></div>`+segments([['cover','填满裁切'],['contain','完整显示']],S.fit,'photo-fit'))+
   section('单格比例',segments([['auto','自动'],['1:1','1:1'],['4:3','4:3'],['3:4','3:4']],S.collageRatio,'cell-ratio'))+
   section('输出与留白',num('长边',2048,'px','collage-edge',256,8192)+`<label class="field"><span>间距</span><input type="number" list="gaps" min="0" max="80" value="${S.gap}" data-value="gap" aria-label="拼图间距"><datalist id="gaps"><option value="0"><option value="8"><option value="12"><option value="16"><option value="24"><option value="32"></datalist></label><div class="field"><span>背景</span><div class="color-row"><input type="color" value="${S.bg}" aria-label="拼图背景" data-value="collage-bg"><span class="small">${S.bg.toUpperCase()}</span></div></div>`);
  footer=btn('生成拼图','create-collage','primary');
  const cols=S.collageCount===9?3:S.collageCount===4?2:S.collageDirection==='vertical'?1:S.collageCount;
  const rows=Math.ceil(S.collageCount/cols);
  visual=`<div class="collage-preview" style="grid-template-columns:repeat(${cols},minmax(0,1fr));grid-template-rows:repeat(${rows},minmax(0,1fr));gap:${S.gap}px;padding:${S.gap}px;background:${S.bg}">${S.cells.slice(0,S.collageCount).map((c,i)=>`<div class="collage-cell ${S.cell===i?'selected':''}" data-cell="${i}"><span class="cell-num">${String(i+1).padStart(2,'0')}</span>${scene(c.scene)}</div>`).join('')}</div>`;
  bottom=`<div class="filmstrip" aria-label="拼图照片">${S.cells.slice(0,S.collageCount).map((c,i)=>`<button draggable="true" data-thumb="${i}" class="${S.cell===i?'selected':''}" data-action="cell:${i}" aria-label="照片 ${i+1}">${scene(c.scene)}</button>`).join('')}<button class="add-cell" data-action="add-collage" aria-label="添加拼图照片">${ico('plus')}</button></div>`;
 }else if(tool==='ocr'||tool==='spark'){
  body=section('识别方式',segments([['ocr','文字 OCR'],['spark','多模态识图']],tool,'tool'))+
   section(tool==='ocr'?'识别语言':'识图任务',tool==='ocr'?choice('语言',['自动识别','简体中文','English'],'ocr-language'):choice('任务',['提取文字','理解图片','翻译内容'],'vision-task')+
   `<textarea aria-label="识图补充要求" data-value="prompt" placeholder="补充要求（可选）"></textarea><div class="info-row"><span>自定义模型</span>${btn('服务设置','settings:vision','')}</div>`)+
   section('识别结果',S.result?`<div class="ocr-result" contenteditable="true" role="textbox" aria-label="识别结果">${esc(S.result).replace(/\n/g,'<br>')}</div>`:'<div class="inspector-status">结果将显示在此处</div>');
  footer=btn(S.task==='ocr'?'正在识别…':'开始识别','recognize','primary',S.task==='ocr'?'disabled':'')+ib('copy','复制识别结果','copy',S.result?'':'disabled');
 }else if(tool==='crop'){
  body=section('裁剪比例',segments([['free','自由'],['1:1','1:1'],['4:3','4:3'],['16:9','16:9']],S.ratio,'ratio'))+
   section('裁剪尺寸',num('宽度',1376,'px','crop-width',1,1600)+num('高度',875,'px','crop-height',1,1067))+
   section('调整',`<div class="row">${ib('refresh','重置裁剪框','reset-crop')}${ib('redo','旋转 90°','rotate-image')}<span class="spacer"></span><span class="small muted">1600 × 1067</span></div>`);
  footer=btn('取消','tool:select','secondary')+btn('应用裁剪','apply-crop','primary');
 }else if(tool==='select'){
  body=section('图片',`<div class="selection-heading">${ico('image')}产品说明.png</div><div class="selection-meta">1600 × 1067 · PNG</div>`)+
   section('画布',num('宽度',1600,'px','image-width',1,8192)+num('高度',1067,'px','image-height',1,8192))+
   section('常用操作',`<div class="row">${ib('crop','裁剪图片','tool:crop')}${ib('redo','旋转图片','rotate-image')}${ib('ocr','识别文字','tool:ocr')}${ib('spark','多模态识图','tool:spark')}</div>`);
 }else{
  body=section(tools.find(t=>t[0]===tool)?.[1]||'标注',`<div class="field"><span>${tool==='text'?ico('text'):'●'}</span><span class="unit"><input aria-label="${tool==='text'?'字号':'粗细'}" type="number" value="${tool==='text'?24:3}" min="1" max="96">${tool==='text'?'pt':'px'}</span></div>`)+
   section('颜色',`<div class="color-row">${['#d16c55','#a8c0e3','#f3c879','#e5e7eb'].map((c,i)=>`<button class="swatch ${i===0?'selected':''}" style="--swatch:${c}" data-action="swatch:${c}" aria-label="颜色 ${c}"></button>`).join('')}<input type="color" value="#d16c55" aria-label="自定义标注颜色"></div>`)+
   section('样式',segments([['line','描边'],['fill','填充']],S.shapeStyle||'line','shape-style')+num('不透明度',100,'%','opacity',0,100));
 }
 if(!visual)visual=S.hasImage?`<div class="image-paper">${paper()}${S.annotated?'<div class="annotation-box"></div>':''}${tool==='crop'?cropOverlay():''}</div>`:`<div class="empty-stage">${ico('image')}<h2>开始处理一张图片</h2><p>截图或打开图片，直接进入当前工作区</p><div class="empty-actions">${ib('capture','区域截图','capture')}${ib('folder','打开图片','open-image')}</div></div>`;
 return `<section class="workbench"><div class="preview-zone"><div class="stage-row">${rail}<div class="canvas" id="canvas"><div class="stage-content">${visual}</div></div></div>${bottom}<div class="transport"><span class="small muted">${tool==='collage'?'拼图 · '+S.collageCount+' 张图片':'1600 × 1067 px'}</span><span class="spacer"></span>${ib('copy','复制图片','copy')}${ib('save','保存图片','save')}</div></div>${inspector('',body,footer,'image-inspector')}</section>`;
}
function library(){
 const side=S.bin?`<nav class="library-sidebar"><h3>素材库</h3>${[['all','全部素材','grid'],['image','图片与截图','image'],['video','视频','video'],['audio','声音','sound'],['project','项目','folder']].map(([v,l,i])=>btn(ico(i)+l+`<span>${v==='all'?assets.length:assets.filter(a=>a.type===v).length}</span>`,'filter:'+v,S.filter===v?'selected':'')).join('')}<h3 style="margin-top:27px">位置</h3>${btn(ico('folder')+'本地素材','directory','')}</nav>`:'';
 const a=assets[S.asset],audio=a.type==='audio';
 const body=section('',`<div class="library-preview">${audio?wave():scene(a.scene)}${a.type==='video'?ib(S.assetPreview?'pause':'play','播放素材预览','asset-play','style="position:absolute;left:calc(50% - 15px);top:calc(50% - 15px);background:var(--panel)"'):''}</div><div class="selection-heading">${esc(a.name)}</div><div class="inspector-status">${a.meta}</div>`)+
 section('文件信息',`<dl class="detail-list"><dt>类型</dt><dd>${{video:'MP4 视频',image:'PNG 图片',audio:'音频文件',project:'Panzo 项目'}[a.type]}</dd><dt>修改时间</dt><dd>今天 14:32</dd><dt>大小</dt><dd>${a.type==='video'?'28.6 MB':'1.2 MB'}</dd><dt>位置</dt><dd><div class="path-line">${ib('folder','打开所在文件夹','directory')}<span class="truncate">D:\\素材\\Panzo</span></div></dd></dl>`);
 return `<section class="workbench">${side}<div class="library-content"><div class="library-toolbar"><div class="search">${ico('search')}<input type="search" placeholder="搜索素材" aria-label="搜索素材" data-value="asset-search" value="${esc(S.query)}"></div><span class="spacer"></span><span class="sort-label small muted">最近修改</span>${ib('grid','缩略图视图','asset-view:grid',`aria-pressed="${S.assetView==='grid'}"`)}${ib('list','列表视图','asset-view:list',`aria-pressed="${S.assetView==='list'}"`)}</div><div class="asset-grid ${S.assetView==='list'?'list':''}" id="asset-grid">${assetCards()}</div></div>${inspector('',body,btn(a.type==='image'?'在图片工具中打开':a.type==='project'?'继续编辑项目':'添加到当前项目','use-asset','primary'))}</section>`;
}
function assetCards(){
 const list=assets.map((a,i)=>({...a,i})).filter(a=>(S.filter==='all'||S.filter===a.type)&&a.name.toLowerCase().includes(S.query.toLowerCase()));
 return list.length?list.map(a=>`<button class="asset-card ${S.asset===a.i?'selected':''}" data-action="asset:${a.i}" data-asset="${a.i}" aria-label="${esc(a.name)}"><div class="asset-thumb">${a.type==='audio'?wave():scene(a.scene)}<span class="asset-type">${{video:'00:24',image:'PNG',audio:'音频',project:'项目'}[a.type]}</span></div><div class="asset-name">${esc(a.name)}</div><div class="asset-info">${a.meta}</div></button>`).join(''):'<div class="library-empty">没有符合条件的素材</div>';
}
function clock(t){return `${String(Math.floor(t/60)).padStart(2,'0')}:${(t%60).toFixed(3).padStart(6,'0')}`;}
function updatePreview(){
 const el=$('.stage-content');if(el)el.style.transform=`translate(${S.pan.x}px,${S.pan.y}px) scale(${S.zoom})`;
 const label=$('.zoom-label');if(label)label.textContent=Math.round(S.zoom*100)+'%';
 const ph=$('.playhead');if(ph)ph.style.left=`calc(112px + (100% - 124px) * ${S.time/24})`;
 const tc=$('.timecode');if(tc)tc.innerHTML=clock(S.time)+' <span class="muted">/ 00:24.000</span>';
 $$('.collage-cell').forEach((cell,i)=>{const s=S.cells[i];$('svg',cell).setAttribute('preserveAspectRatio',S.fit==='contain'?'xMidYMid meet':'xMidYMid slice');$('.scene',cell).style.transform=`translate(${s.x}px,${s.y}px) scale(${s.z})`;});
 fitCanvas();
}
function fitCanvas(){
 const canvas=$('#canvas');if(!canvas)return;
 const r=canvas.getBoundingClientRect(), w=Math.max(60,r.width-50),h=Math.max(60,r.height-50);
 const frame=$('.media-frame')||$('.image-paper')||$('.collage-preview');
 if(!frame)return;
 let ratio=frame.classList.contains('image-paper')?1.5:16/9;
 if(frame.classList.contains('collage-preview')){
  let [a,b]=(S.collageRatio==='auto'?'1:1':S.collageRatio).split(':').map(Number);
  const cols=S.collageCount===9?3:S.collageCount===4?2:S.collageDirection==='vertical'?1:S.collageCount;
  const rows=Math.ceil(S.collageCount/cols),gap=S.gap;
  const cellHeight=Math.max(10,Math.min((h-gap*(rows+1))/rows,(Math.min(w,780)-gap*(cols+1))/cols/(a/b)));
  frame.style.width=(cols*cellHeight*(a/b)+gap*(cols+1))+'px';
  frame.style.height=(rows*cellHeight+gap*(rows+1))+'px';frame.style.maxHeight='none';
  return;
 }else if(S.canvasRatio&&S.page==='video'){let [a,b]=S.canvasRatio.split(':').map(Number);ratio=a/b;}
 const fw=Math.min(w,h*ratio,1100);
 frame.style.width=fw+'px';frame.style.height=fw/ratio+'px';frame.style.maxHeight='none';
}
function updateTask(){
 const el=$('#background-task');
 el.hidden=!S.recording&&!S.task;
 el.innerHTML=S.recording?`<span class="record-circle"></span><button data-action="return-task">旁白 ${clock(S.recordSeconds)}</button>${ib('stop','结束录音','stop-recording')}`:
 S.task?`${ico('clock')}<button data-action="return-task">${S.task==='ocr'?'图片识别中':'字幕生成中'}</button>${ib('close','取消任务','cancel-task')}`:'';
}
function modal(title,body,footer=''){
 const dialog=$('#dialog');
 $('#dialog-content').innerHTML=`<header class="dialog-head"><h2 id="dialog-title">${title}</h2>${ib('close','关闭窗口','close-dialog')}</header>${body}<footer class="dialog-footer">${footer||btn('关闭','close-dialog','secondary')}</footer>`;
 if(!dialog.open)dialog.showModal();
}
function settings(tab='general'){
 S.settingsTab=tab;
 const nav=[['general','外观与行为'],['vision','识图服务'],['speech','AI 字幕'],['keys','快捷键']];
 let fields='';
 if(tab==='general')fields=section('界面',choice('主题',['跟随当前选择','深色','浅色'],'theme-choice')+choice('界面动画',['跟随系统','关闭'],'motion'))+section('文件与窗口',`<div class="field"><span>素材目录</span><div class="path-line">${ib('folder','选择素材目录','directory')}<span data-action="edit-path" tabindex="0">D:\\素材\\Panzo</span></div></div>`+choice('关闭主窗口',['保留到托盘','退出应用'],'close-mode'));
 else if(tab==='keys')fields=section('全局快捷键',`<div class="field"><span>区域截图</span><input value="Ctrl+Alt+Shift+F9" aria-label="区域截图快捷键"></div><div class="field"><span>录屏 / 停止</span><input value="Ctrl+Alt+Shift+F10" aria-label="录屏快捷键"></div>`)+section('编辑快捷键',`<div class="field"><span>分割片段</span><input value="B" aria-label="分割片段快捷键"></div><div class="field"><span>删除片段</span><input value="Delete" aria-label="删除片段快捷键"></div>`);
 else fields=section(tab==='speech'?'语音模型':'多模态模型',`<label class="field"><span>服务地址</span><input aria-label="服务地址" value="${esc(S.endpoint||'https://api.example.com/v1')}" data-value="endpoint"></label><label class="field"><span>模型</span><input aria-label="模型" value="${esc(S.model||(tab==='speech'?'speech-model':'vision-model'))}" data-value="model"></label><label class="field"><span>API Key</span><input aria-label="API Key" type="password" placeholder="填写自己的 API Key" autocomplete="off"></label>`)+section('识别偏好',choice('语言',['自动','简体中文','English'],'language')+num('超时',120,'秒','timeout',10,600));
 modal('设置',`<div class="settings-layout"><nav class="settings-nav">${nav.map(([v,l])=>btn(l,'settings:'+v,S.settingsTab===v?'selected':'')).join('')}</nav><div class="settings-content">${fields}</div></div>`,btn('取消','close-dialog','secondary')+btn('保存','save-settings','primary'));
}
function plugins(){
 modal('插件',`<div class="dialog-body"><div class="row between"><span class="small muted">按需启用功能</span>${btn('安装本地插件','install-plugin','secondary')}</div><div class="plugin-row">${ico('mic')}<div class="plugin-copy"><strong>声音旁白与 AI 字幕</strong><p>录制旁白、音频导入、字幕校对与自动生成</p><p>${S.recording||S.task==='subtitles'?'正在使用 · 任务结束后可停用':S.plugin?'已启用 · 独立插件':'已停用 · 已有作品保留'}</p></div><button class="toggle ${S.plugin?'on':''}" data-action="toggle-plugin" aria-label="启用声音与字幕插件" aria-pressed="${S.plugin}" ${S.recording||S.task==='subtitles'?'disabled':''}></button></div>${[['ocr','离线文字 OCR','系统文字识别 · 内置功能'],['spark','多模态识图','自定义视觉模型 · 内置功能'],['collage','拼图与批处理','排版与图像批处理 · 内置功能']].map(([icon,title,desc])=>`<div class="plugin-row">${ico(icon)}<div class="plugin-copy"><strong>${title}</strong><p>${desc}</p></div><span class="small muted">已启用</span></div>`).join('')}</div>`);
}
function recordDialog(){
 modal('区域录屏',`<div class="dialog-body"><div class="record-mode">${[['capture','region','区域'],['window','window','窗口'],['video','screen','屏幕']].map(([i,v,l])=>btn(ico(i)+l,'record-mode:'+v,(S.recordMode||'region')===v?'selected':'')).join('')}</div><div class="record-area">${ico('capture')}<span>${S.recordMode==='window'?'Panzo · 当前窗口':S.recordMode==='screen'?'显示器 1 · 2560 × 1440':'已选区域 · 1920 × 1080'}</span>${btn('重新选择区域','select-record-area','secondary')}</div>${choice('帧率',['30 fps','60 fps','24 fps'],'fps')}${choice('声音',['系统声音','麦克风','系统声音 + 麦克风','无声音'],'record-source')}<div class="field"><span>鼠标跟踪缩放</span><button class="toggle ${S.follow?'on':''}" data-action="follow-record" aria-label="鼠标跟踪缩放" aria-pressed="${!!S.follow}"></button></div></div>`,btn('取消','close-dialog','secondary')+btn('开始录制','screen-record','primary'));
}
function exportDialog(){
 modal('导出',`<div class="dialog-body"><div class="selection-heading">${ico(S.page==='image'?'image':'video')}${S.page==='image'?'产品说明':'山间手记'}</div><div style="height:15px"></div>${choice('格式',S.page==='image'?['PNG','JPG','WebP']:['MP4','GIF'],'format')}${choice('分辨率',['原始尺寸','1920 × 1080','1280 × 720'],'export-resolution')}${choice('帧率',['30 fps','60 fps','24 fps','15 fps','10 fps'],'export-fps')}<div class="field"><span>输出位置</span><div class="path-line">${ib('folder','选择输出目录','directory')}<span data-action="edit-path" tabindex="0">D:\\素材\\Panzo\\导出</span></div></div></div>`,btn('取消','close-dialog','secondary')+btn('导出','confirm-export','primary'));
}
function about(){
 modal('布局交互原型',`<div class="dialog-body"><p class="prototype-note">这是一份可操作的布局设计稿，保留 Panzo 当前主题。截图、录音、AI 识别及导出使用演示数据，不访问设备、模型或真实工程。</p><p class="prototype-note">可体验：三个工作区切换、选中轨道更新属性、分割/删除/撤销、字幕校对、面板折叠、拖动分隔线、Ctrl + 滚轮缩放、拖动画面、拼图单格定位和照片排序。</p><p class="prototype-note">设置、插件、录制和导出使用弹窗。菜单“视图”可以切换图片空状态。原生实现方案见同目录 README 和项目 docs 中的布局评审。</p></div>`);
}
function menu(type,anchor){
 const m=$('#flyout');
 const rows={
 file:[['folder','打开…','open-image','Ctrl+O'],['record','区域录屏','record',''],['save','保存','save','Ctrl+S'],['download','导出…','export','']],
 edit:[['undo','撤销','undo','Ctrl+Z'],['cut','分割片段','split','B'],['trash','删除所选片段','delete-clip','Delete']],
 view:[['panel','显示 / 隐藏属性面板','inspector',''],['fit','预览居中并适应窗口','fit',''],['image','查看图片空状态','empty-image',''],['sun','切换主题','theme','']],
 image:[['capture','区域截图','capture',''],['folder','打开图片','open-image',''],['collage','拼图','tool:collage',''],['close','关闭当前图片','close-image','']],
 bin:[['folder','从素材库添加','open-library',''],['record','区域录屏','record','']]
 };
 m.innerHTML=(rows[type]||rows.file).map(([i,l,a,k])=>`<button data-action="${a}">${ico(i)}${l}<span class="shortcut">${k}</span></button>`).join('');
 const r=anchor.getBoundingClientRect();m.hidden=false;m.style.left=Math.min(r.left,innerWidth-250)+'px';m.style.top=Math.min(r.bottom+4,innerHeight-200)+'px';
}
function navigate(page){S.views??={};S.views[S.page]={zoom:S.zoom,pan:{...S.pan}};S.page=page;const view=S.views[page]||{zoom:1,pan:{x:0,y:0}};S.zoom=view.zoom;S.pan={...view.pan};render();}
function action(key,el){
 const [a,...rest]=key.split(':'),v=rest.join(':');
 switch(a){
 case 'theme':theme();break;
 case 'about':about();break;
 case 'window-demo':toast('原型窗口：请使用浏览器的窗口控制');break;
 case 'close-dialog':$('#dialog').close();break;
 case 'settings':settings(v||'general');break;
 case 'save-settings':$('#dialog').close();toast('已保存本次原型会话的设置');break;
 case 'plugins':plugins();break;
 case 'toggle-plugin':if(S.recording||S.task==='subtitles')break;S.plugin=!S.plugin;render();plugins();break;
 case 'install-plugin':toast('原型演示：正式应用在此选择 .panzoplugin 文件');break;
 case 'inspector':S.inspector=!S.inspector;render();break;
 case 'bin':if(S.page==='video'&&innerWidth<=1000)S.compactBin=!S.compactBin;else S.bin=!S.bin;render();break;
 case 'bin-menu':menu('bin',el);break;
 case 'menu-image':menu('image',el);break;
 case 'open-library':navigate('assets');break;
 case 'video-tab':S.videoTab=v;S.inspector=true;render();break;
 case 'tool':S.imageTool=v;S.hasImage=true;S.inspector=true;if(S.page!=='image')navigate('image');else render();break;
 case 'capture':case 'open-image':S.hasImage=true;S.modified=a==='capture';S.imageTool='select';navigate('image');toast(a==='capture'?'已将演示截图送入图片工作区':'已打开演示图片');break;
 case 'image-info':S.imageTool='select';S.inspector=true;render();break;
 case 'close-image':case 'empty-image':S.hasImage=false;S.imageTool='select';navigate('image');break;
 case 'fit':S.zoom=1;S.pan={x:0,y:0};updatePreview();break;
 case 'play':S.play=!S.play;render();break;
 case 'first':S.time=0;updatePreview();break;
 case 'last':S.time=24;S.play=false;render();break;
 case 'mute':el.classList.toggle('selected');el.innerHTML=ico(el.classList.contains('selected')?'mute':'sound');break;
 case 'mute-track':el.classList.toggle('selected');el.setAttribute('aria-pressed',el.classList.contains('selected'));break;
 case 'clip':{
  S.selectedClip=v;const c=S.clips.find(c=>c.id===v);S.inspector=true;
  if(c?.kind==='subtitle'){S.selectedSubtitle=Math.max(0,S.clips.filter(c=>c.kind==='subtitle').findIndex(c=>c.id===v));S.videoTab='subtitle';}else S.videoTab='clip';
  render();break;}
 case 'split':{
  const c=S.clips.find(c=>c.id===S.selectedClip);
  if(!c||S.time<=c.start+.1||S.time>=c.end-.1){toast('将播放头移到所选片段内部再分割');break;}
  savePoint();const end=c.end;c.end=S.time;S.clips.push({...c,id:c.id+'b'+S.undo.length,start:S.time,end,name:c.name+' · 02'});render();break;}
 case 'delete-clip':if(S.selectedClip){savePoint();S.clips=S.clips.filter(c=>c.id!==S.selectedClip);S.selectedClip=null;render();}break;
 case 'undo':if(S.undo.length){Object.assign(S,JSON.parse(S.undo.pop()));S.modified=true;render();}else toast('没有可撤销的修改');break;
 case 'speed':savePoint();S.speed=Number(v);render();break;
 case 'ratio':S.ratio=v;if(v!=='free'){const [w,h]=v.split(':').map(Number);S.crop.w=80;S.crop.h=Math.min(86,80*1.5/(w/h));S.crop.x=10;S.crop.y=(100-S.crop.h)/2;}render();break;
 case 'video-crop':S.videoCropping=!S.videoCropping;render();break;
 case 'reset-crop':S.crop={x:7,y:8,w:86,h:82};S.ratio='free';S.videoCropping=false;render();break;
 case 'apply-crop':savePoint();S.imageTool='select';S.cropped=true;render();toast('演示裁剪已应用，可撤销');break;
 case 'rotate-image':savePoint();S.rotated=!S.rotated;render();if($('.image-paper'))$('.image-paper').style.transform=S.rotated?'rotate(90deg)':'none';break;
 case 'shape-style':S.shapeStyle=v;render();break;
 case 'swatch':$$('.swatch').forEach(b=>b.classList.toggle('selected',b===el));break;
 case 'follow':S.follow=!S.follow;render();break;
 case 'follow-record':S.follow=!S.follow;recordDialog();break;
 case 'canvas-ratio':S.canvasRatio=v;render();break;
 case 'recognize':
  if(S.task)return;S.task='ocr';render();
  S.taskTimer=setTimeout(()=>{S.task=null;S.result='让灵感，有迹可循。\n\n记录眼前的画面\n整理片刻的思绪\n把想法变成完整的表达\n\n捕捉 · 标注 · 创作';render();},1800);break;
 case 'ai-subtitle':
  if(S.task||!S.plugin)return;S.task='subtitles';render();
  S.taskTimer=setTimeout(()=>{S.task=null;render();toast('演示字幕已生成，可在右侧校对');},2200);break;
 case 'cancel-task':clearTimeout(S.taskTimer);S.task=null;render();break;
 case 'return-task':if(S.recording||S.task==='subtitles'){S.videoTab=S.recording?'audio':'subtitle';navigate('video');}else{S.imageTool='ocr';navigate('image');}S.inspector=true;render();break;
 case 'start-recording':if(!S.plugin)break;S.recording=true;S.recordSeconds=0;render();break;
 case 'stop-recording':{
  if(!S.recording)break;savePoint();S.clips.push({id:'a'+Date.now(),kind:'audio',start:S.time,end:Math.min(24,S.time+Math.max(2,S.recordSeconds)),name:'新旁白'});S.recording=false;render();toast('演示旁白已添加到轨道');break;}
 case 'refresh-device':toast('已刷新演示录音设备');break;
 case 'record':recordDialog();break;
 case 'record-mode':S.recordMode=v;recordDialog();break;
 case 'select-record-area':toast('原型演示区域：1920 × 1080');break;
 case 'screen-record':$('#dialog').close();S.videoTab='audio';navigate('video');toast('区域录屏流程演示：结束后返回当前视频工作台');break;
 case 'import-audio':savePoint();S.clips.push({id:'a'+Date.now(),kind:'audio',start:4,end:15,name:'已导入的演示音频'});render();toast('音频已加入轨道，可在轨道中选中编辑');break;
 case 'import-subtitle':case 'add-subtitle':savePoint();S.subtitles.push('新字幕');S.selectedSubtitle=S.subtitles.length-1;S.clips.push({id:'s'+Date.now(),kind:'subtitle',start:Math.min(20,S.time),end:Math.min(24,S.time+3),name:'新字幕'});render();break;
 case 'subtitle':S.selectedSubtitle=Number(v);S.time=Number(v)*6+2;render();break;
 case 'apply-subtitle':{
  const text=$('[data-value="subtitle-text"]')?.value||'';if(!text.trim()){toast('字幕内容不能为空');break;}
  savePoint();S.subtitles[S.selectedSubtitle]=text;const c=S.clips.filter(c=>c.kind==='subtitle')[S.selectedSubtitle];if(c)c.name=text;render();toast('修改已应用，可撤销');break;}
 case 'export-srt':toast('原型演示：正式应用在此导出 SRT 文件');break;
 case 'export':exportDialog();break;
 case 'confirm-export':$('#dialog').close();toast('导出流程演示完成，原型不生成媒体文件');break;
 case 'save':S.modified=false;renderContext();toast('原型会话已保存，未写入真实工程');break;
 case 'copy':toast('复制入口演示：正式应用在此写入剪贴板');break;
 case 'collage':S.collageCount=Number(v);S.cell=Math.min(S.cell,S.collageCount-1);render();break;
 case 'direction':S.collageDirection=v;render();break;
 case 'photo-fit':S.fit=v;render();$$('.collage-cell svg').forEach(s=>s.setAttribute('preserveAspectRatio',v==='cover'?'xMidYMid slice':'xMidYMid meet'));break;
 case 'cell-ratio':S.collageRatio=v;render();break;
 case 'cell':S.cell=Number(v);$$('.collage-cell').forEach((c,i)=>c.classList.toggle('selected',i===S.cell));$$('[data-thumb]').forEach((c,i)=>c.classList.toggle('selected',i===S.cell));break;
 case 'add-collage':modal('添加拼图照片',`<div class="dialog-body"><p class="prototype-note">当前布局已放满 ${S.collageCount} 张照片。可以替换选中照片，或选择九宫格继续排版。</p><div class="row">${btn('替换选中照片','replace-collage','secondary')}${btn('切换九宫格','expand-collage','primary')}</div></div>`);break;
 case 'replace-collage':S.cells[S.cell].scene=(S.cells[S.cell].scene+1)%4;$('#dialog').close();render();break;
 case 'expand-collage':S.collageCount=9;$('#dialog').close();render();break;
 case 'create-collage':savePoint();S.imageTool='select';render();toast('拼图生成流程完成，演示稿保留在当前会话');break;
 case 'filter':S.filter=v;render();break;
 case 'asset':S.asset=Number(v);render();break;
 case 'asset-view':S.assetView=v;render();break;
 case 'use-asset':if(assets[S.asset].type==='image'){S.imageTool='select';S.hasImage=true;navigate('image');}else{navigate('video');toast('已将演示素材送入当前项目');}break;
 case 'asset-play':S.assetPreview=!S.assetPreview;el.innerHTML=ico(S.assetPreview?'pause':'play');toast(S.assetPreview?'正在预览演示素材':'预览已暂停');break;
 case 'import':modal('添加素材',`<div class="dialog-body"><div class="record-area">${ico('folder')}<span>选择图片、视频或声音文件</span>${btn('使用演示素材','demo-import','primary')}</div></div>`);break;
 case 'demo-import':$('#dialog').close();toast('演示素材已添加到项目素材区');break;
 case 'directory':toast('原型演示：正式应用打开系统目录选择器；路径文字可双击编辑');break;
 case 'edit-path':{
  if(el.tagName==='INPUT')break;const text=el.textContent;el.innerHTML=`<input aria-label="手动输入目录" value="${esc(text)}" data-path-edit>`;const input=$('input',el);input.focus();input.select();input.addEventListener('blur',()=>{el.textContent=input.value||text;},{once:true});break;}
 case 'refresh-assets':toast('素材列表已刷新');break;
 case 'timeline-in':case 'timeline-out':{
  const g=$('.timeline-grid');S.tz=Math.max(1,Math.min(3,(S.tz||1)+(a==='timeline-in'?.25:-.25)));g.style.width=S.tz*100+'%';break;}
 case 'timeline-fit':S.tz=1;$('.timeline-grid').style.width='100%';break;
 }
}
document.addEventListener('click',e=>{
 const page=e.target.closest('[data-page]');if(page?.tagName==='BUTTON'){navigate(page.dataset.page);return;}
 const menuEl=e.target.closest('[data-menu]');if(menuEl){menu(menuEl.dataset.menu,menuEl);return;}
 const b=e.target.closest('[data-action]');if(b&&!b.disabled){if(b.dataset.action!=='menu-image'&&b.dataset.action!=='bin-menu')$('#flyout').hidden=true;action(b.dataset.action,b);}
 else if(!e.target.closest('#flyout'))$('#flyout').hidden=true;
});
document.addEventListener('dblclick',e=>{const card=e.target.closest('[data-asset]');if(card){S.asset=Number(card.dataset.asset);action('use-asset');}const p=e.target.closest('.path-line');if(p&&!e.target.closest('button,input'))action('edit-path',p);});
document.addEventListener('input',e=>{
 const k=e.target.dataset.value;if(!k)return;
 if(k==='asset-search'){S.query=e.target.value;$('#asset-grid').innerHTML=assetCards();return;}
 if(k==='bin-search'){$$('.bin-item').forEach(el=>el.hidden=!el.textContent.toLowerCase().includes(e.target.value.toLowerCase()));return;}
 if(k==='gap'){S.gap=Math.max(0,Math.min(80,Number(e.target.value)));const p=$('.collage-preview');if(p){p.style.gap=S.gap+'px';p.style.padding=S.gap+'px';fitCanvas();}return;}
 if(k==='collage-bg'){S.bg=e.target.value;$('.collage-preview').style.background=S.bg;return;}
 if(k==='speed'){S.speed=Math.max(.25,Math.min(4,Number(e.target.value)));$$('[data-value=speed]').forEach(i=>i.value=S.speed);return;}
 if(k==='subtitle-text'){const p=$('#subtitle-preview');if(p)p.textContent=e.target.value;return;}
 if(k==='timeline-zoom'){S.tz=1+Number(e.target.value)/50;$('.timeline-grid').style.width=S.tz*100+'%';return;}
 if(k==='endpoint'||k==='model')S[k]=e.target.value;
 if(k==='canvas-color')$('#canvas').style.background=e.target.value;
 if(k==='motion')document.documentElement.dataset.motion=e.target.value==='关闭'?'off':'on';
 if(k==='theme-choice'&&e.target.value!=='跟随当前选择'){document.documentElement.dataset.theme=e.target.value==='浅色'?'light':'dark';updateTheme();}
 if(k==='clip-start'||k==='clip-end'){const c=S.clips.find(c=>c.id===S.selectedClip);if(c){const n=Number(e.target.value);if(k==='clip-start'&&n<c.end)c.start=Math.max(0,n);if(k==='clip-end'&&n>c.start)c.end=Math.min(24,n);S.modified=true;}}
});
let tipTimer;
document.addEventListener('pointerover',e=>{const b=e.target.closest('[data-tip]');clearTimeout(tipTimer);if(!b)return;tipTimer=setTimeout(()=>{const r=b.getBoundingClientRect(),t=$('#tooltip');t.textContent=b.dataset.tip;t.hidden=false;t.style.left=Math.min(r.left,innerWidth-t.offsetWidth-8)+'px';t.style.top=(r.bottom+8+t.offsetHeight<innerHeight?r.bottom+8:r.top-t.offsetHeight-8)+'px';},450);});
document.addEventListener('pointerout',()=>{clearTimeout(tipTimer);$('#tooltip').hidden=true;});
document.addEventListener('keydown',e=>{
 const typing=e.target.matches('input,textarea,[contenteditable]');if(e.key==='Escape'){if($('#dialog').open)$('#dialog').close();$('#flyout').hidden=true;return;}
 if(typing){if(e.key==='Enter'&&e.target.hasAttribute('data-path-edit'))e.target.blur();return;}
 if(e.key===' '&&S.page==='video'){e.preventDefault();action('play');}
 if(e.key.toLowerCase()==='b'&&S.page==='video')action('split');
 if(e.key==='Delete'&&S.page==='video')action('delete-clip');
 if(e.ctrlKey&&e.key.toLowerCase()==='z'){e.preventDefault();action('undo');}
 if(e.ctrlKey&&e.key.toLowerCase()==='s'){e.preventDefault();action('save');}
 if(e.target.matches('.timeline-divider')&&['ArrowUp','ArrowDown'].includes(e.key)){e.preventDefault();S.timelineHeight=Math.max(150,Math.min(innerHeight*.55,($('.timeline').offsetHeight)+(e.key==='ArrowUp'?20:-20)));$('.timeline').style.height=S.timelineHeight+'px';$('.timeline').classList.toggle('compact',S.timelineHeight<200);fitCanvas();}
 if(e.target.matches('.splitter-vertical')&&['ArrowLeft','ArrowRight'].includes(e.key)){e.preventDefault();const width=parseFloat(getComputedStyle($('.inspector')).width)+(e.key==='ArrowLeft'?16:-16);document.documentElement.style.setProperty('--inspector',Math.max(264,Math.min(420,width))+'px');fitCanvas();}
});
function bindStage(){
 const canvas=$('#canvas');if(canvas)canvas.addEventListener('wheel',e=>{
  const cell=e.target.closest('.collage-cell');
  if(cell){e.preventDefault();const i=Number(cell.dataset.cell);S.cell=i;S.cells[i].z=Math.max(1,Math.min(4,S.cells[i].z+(e.deltaY<0?.12:-.12)));updatePreview();return;}
  if(e.ctrlKey){e.preventDefault();S.zoom=Math.max(.25,Math.min(4,S.zoom*(e.deltaY<0?1.1:1/1.1)));updatePreview();}
 },{passive:false});
}
let drag=null, draggedThumb=null;
document.addEventListener('dragstart',e=>{const thumb=e.target.closest('[data-thumb]');if(thumb){draggedThumb=Number(thumb.dataset.thumb);e.dataTransfer.setData('text/plain',thumb.dataset.thumb);}});
document.addEventListener('dragover',e=>{if(e.target.closest('[data-thumb]'))e.preventDefault();});
document.addEventListener('drop',e=>{const thumb=e.target.closest('[data-thumb]');if(thumb&&draggedThumb!==null){e.preventDefault();const to=Number(thumb.dataset.thumb);[S.cells[to],S.cells[draggedThumb]]=[S.cells[draggedThumb],S.cells[to]];S.cell=to;draggedThumb=null;render();}});
document.addEventListener('pointerdown',e=>{
 if(e.button!==0)return;
 const timeline=e.target.closest('.timeline-divider'),divider=e.target.closest('.splitter-vertical'),crop=e.target.closest('.crop-box'),cell=e.target.closest('.collage-cell'),ruler=e.target.closest('.time-ruler'),canvas=e.target.closest('#canvas');
 if(timeline)drag={kind:'timeline',y:e.clientY,height:$('.timeline').offsetHeight};
 else if(divider)drag={kind:'divider',x:e.clientX,width:$('.inspector').offsetWidth};
 else if(crop)drag={kind:'crop',x:e.clientX,y:e.clientY,rect:crop.parentElement.getBoundingClientRect(),c:{...S.crop},corner:e.target.dataset.corner,el:crop};
 else if(cell){const i=Number(cell.dataset.cell);action('cell:'+i,cell);drag={kind:'cell',i,x:e.clientX,y:e.clientY,start:{...S.cells[i]}};}
 else if(ruler){const r=ruler.getBoundingClientRect();S.time=Math.max(0,Math.min(24,(e.clientX-r.left)/r.width*24));updatePreview();drag={kind:'ruler',r};}
 else if(canvas&&!e.target.closest('button,input')&&(!['pen','rectangle','arrow'].includes(S.imageTool)||S.page==='video'))drag={kind:'pan',x:e.clientX,y:e.clientY,start:{...S.pan}};
 else if(canvas&&S.page==='image'){S.annotated=true;S.modified=true;const p=$('.image-paper');if(p&&!$('.annotation-box',p))p.insertAdjacentHTML('beforeend','<div class="annotation-box"></div>');}
 if(drag){e.preventDefault();document.body.style.userSelect='none';}
});
document.addEventListener('pointermove',e=>{
 if(!drag)return;const d=drag;
 if(d.kind==='timeline'){S.timelineHeight=Math.max(150,Math.min(innerHeight*.55,d.height+d.y-e.clientY));$('.timeline').style.height=S.timelineHeight+'px';$('.timeline').classList.toggle('compact',S.timelineHeight<200);fitCanvas();}
 if(d.kind==='divider'){document.documentElement.style.setProperty('--inspector',Math.max(264,Math.min(420,d.width+d.x-e.clientX))+'px');fitCanvas();}
 if(d.kind==='pan'){S.pan={x:d.start.x+e.clientX-d.x,y:d.start.y+e.clientY-d.y};updatePreview();}
 if(d.kind==='cell'){const rect=$$('.collage-cell')[d.i].getBoundingClientRect(),lx=(S.cells[d.i].z-1)*rect.width/2,ly=(S.cells[d.i].z-1)*rect.height/2;S.cells[d.i].x=Math.max(-lx,Math.min(lx,d.start.x+e.clientX-d.x));S.cells[d.i].y=Math.max(-ly,Math.min(ly,d.start.y+e.clientY-d.y));updatePreview();}
 if(d.kind==='ruler'){S.time=Math.max(0,Math.min(24,(e.clientX-d.r.left)/d.r.width*24));updatePreview();}
 if(d.kind==='crop'){
  const dx=(e.clientX-d.x)/d.rect.width*100,dy=(e.clientY-d.y)/d.rect.height*100,c={...d.c};
  if(!d.corner){c.x=Math.max(0,Math.min(100-c.w,c.x+dx));c.y=Math.max(0,Math.min(100-c.h,c.y+dy));}
  else{if(d.corner.includes('e'))c.w=Math.max(15,Math.min(100-c.x,d.c.w+dx));if(d.corner.includes('s'))c.h=Math.max(15,Math.min(100-c.y,d.c.h+dy));if(d.corner.includes('w')){c.x=Math.max(0,Math.min(d.c.x+d.c.w-15,d.c.x+dx));c.w=d.c.w+d.c.x-c.x;}if(d.corner.includes('n')){c.y=Math.max(0,Math.min(d.c.y+d.c.h-15,d.c.y+dy));c.h=d.c.h+d.c.y-c.y;}}
  if(d.corner&&S.ratio!=='free'){let [rw,rh]=S.ratio.split(':').map(Number);c.h=Math.min(100-c.y,c.w*1.5/(rw/rh));c.w=c.h*(rw/rh)/1.5;if(d.corner.includes('n'))c.y=Math.max(0,d.c.y+d.c.h-c.h);}
  S.crop=c;Object.assign(d.el.style,{left:c.x+'%',top:c.y+'%',width:c.w+'%',height:c.h+'%'});$('.crop-size',d.el).textContent=Math.round(c.w*16)+' × '+Math.round(c.h*10.67);
 }
});
document.addEventListener('pointerup',()=>{drag=null;document.body.style.userSelect='';});
window.addEventListener('resize',fitCanvas);
setInterval(()=>{
 if(S.play){S.time+=.1*S.speed;if(S.time>=24){S.time=24;S.play=false;render();}else updatePreview();}
 if(S.recording){S.recordSeconds+=.1;const time=$('.rec-time');if(time)time.textContent=clock(S.recordSeconds);updateTask();}
},100);
render();
