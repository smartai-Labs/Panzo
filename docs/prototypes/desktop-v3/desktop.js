// Desktop v3 shell. Local design preview; native capture/OCR/export are simulated.
sampleImage = function(variant=0) {
 const title=variant?'交互细节检查表':'区域截图与识别工作流';
 const svg='<svg xmlns="http://www.w3.org/2000/svg" width="960" height="620" viewBox="0 0 960 620"><rect width="960" height="620" fill="#f9f8f3"/><g font-family="Microsoft YaHei,Segoe UI,sans-serif"><text x="65" y="53" font-size="12" fill="#839084">PANZO / 产品评审记录</text><text x="895" y="53" text-anchor="end" font-size="12" fill="#899188">2026.10.08</text><path d="M65 70H895" stroke="#d6d9ce"/><text x="65" y="127" font-size="29" font-weight="600" fill="#304437">'+title+'</text><text x="65" y="160" font-size="13" fill="#7d897f">讨论范围：图片工作区、文字识别与素材流转</text><text x="65" y="216" font-size="17" font-weight="600" fill="#3f4f41">01　工作流约定</text><text x="65" y="252" font-size="15" fill="#555f55">截图完成后，图片回到当前工作区。用户可以继续标注、裁剪，</text><text x="65" y="280" font-size="15" fill="#555f55">或在右侧选择文字识别；切换功能时保留当前内容。</text><g fill="#eef0e7" stroke="#c8d0c0"><rect x="65" y="309" width="230" height="66" rx="2"/><rect x="365" y="309" width="230" height="66" rx="2"/><rect x="665" y="309" width="230" height="66" rx="2"/></g><g fill="#4b624b" font-size="15" text-anchor="middle"><text x="180" y="348">截取 / 导入图片</text><text x="480" y="348">标注与识别</text><text x="780" y="348">校对 / 复制 / 保存</text></g><g fill="none" stroke="#8a9b85" stroke-width="1.5"><path d="M310 342H348M340 336L348 342L340 348"/><path d="M610 342H648M640 336L648 342L640 348"/></g><text x="65" y="430" font-size="17" font-weight="600" fill="#3f4f41">02　待确认事项</text><g fill="none" stroke="#86947f"><rect x="66" y="458" width="13" height="13"/><path d="M68 464L72 468L78 460"/><rect x="66" y="490" width="13" height="13"/><path d="M68 496L72 500L78 492"/><rect x="66" y="522" width="13" height="13"/></g><g font-size="14" fill="#555f55"><text x="92" y="470">识别结果与原图一一对应，切换标签后保留</text><text x="92" y="502">离线 OCR 与多模态识图，在同一面板中切换</text><text x="92" y="534">快捷键和高 DPI 布局，进入下一轮检查</text></g><path d="M65 567H895" stroke="#d6d9ce"/><text x="65" y="590" font-size="11" fill="#899188">设计示例文档 · 用于预览图片与识别面板</text><text x="895" y="590" text-anchor="end" font-size="11" fill="#899188">01 / 01</text></g></svg>';
 return 'data:image/svg+xml;charset=utf-8,'+encodeURIComponent(svg);
};
baseAssets.forEach((a,i)=>{a.url=sampleImage(i%2);a.name=['产品评审记录.png','工作流录制.panzo','交互检查表.png','操作过程.gif'][i]});
pages.plugins[1]='插件';
S.inspectorHidden=false;
S.inspectorTab='recognition';
S.theme='dark';
try { S.theme=localStorage.getItem('panzo-desktop-v3-theme')==='light'?'light':'dark' } catch {}
theme=function(t){S.theme=t;try{localStorage.setItem('panzo-desktop-v3-theme',t)}catch{}render()};
emptyCanvas=function(){return '<div class="empty-stage"><div class="empty-symbol">'+icon('image')+'</div><h2>尚未打开图片</h2><p>将图片拖到这里，或按 Ctrl V 粘贴</p><div class="empty-actions">'+button('打开文件','open-image','folder')+button('区域截图','capture','capture')+'</div></div>'};
const toolNames={select:'选择',crop:'裁剪',pen:'画笔',rect:'矩形',arrow:'箭头',text:'文字',mosaic:'马赛克'};
function toolRail(){
 return '<aside class="tool-rail" aria-label="图片编辑工具">'+Object.entries(toolNames).map(([k,t])=>ib(t,'tool:'+k,k,S.tool===k?'active':'')).join('')+'<i class="separator"></i>'+ib('撤销标注','undo','undo')+ib('贴到桌面','pin','pin')+'<span class="color-swatch" title="标注颜色示意"></span><div class="rail-end">'+ib('显示 / 隐藏识别面板','toggle-inspector','layers',S.inspectorHidden?'':'active')+'</div></aside>';
}
function rulers(){return '<div class="ruler-x" aria-hidden="true">'+[0,100,200,300,400,500,600,700,800,900].map(x=>'<span>'+x+'</span>').join('')+'</div><div class="ruler-y" aria-hidden="true">'+[0,100,200,300,400,500].map(x=>'<span>'+x+'</span>').join('')+'</div>'}
function inspectorTabs(){
 return '<div class="inspector-tabs"><button data-action="inspect:recognition" class="'+(S.inspectorTab==='recognition'?'selected':'')+'">文字识别</button><button data-action="inspect:info" class="'+(S.inspectorTab==='info'?'selected':'')+'">图像信息</button>'+ib('收起面板','toggle-inspector','close')+'</div>';
}
imagePage=function(){
 const d=doc(),show=!!(d||S.panels.image)&&!S.inspectorHidden;
 const tabs=d?'<div class="image-tabs" role="tablist" aria-label="打开的图片">'+S.docs.map(x=>'<button role="tab" aria-selected="'+(x.id===S.active)+'" class="image-tab '+(x.id===S.active?'selected':'')+'" data-doc="'+x.id+'">'+icon('image')+'<span>'+esc(x.name)+(x.annotations.length?' *':'')+'</span></button>').join('')+'</div>':'';
 const context=d?'<div class="document-tools"><span>'+toolNames[S.tool]+'</span><i class="separator"></i><span>'+(['rect','pen'].includes(S.tool)?'拖动以绘制标注':'点击左侧工具进行编辑')+'</span><span class="tool-text">像素单位 · '+d.width+' × '+d.height+'</span></div>':'';
 const stage=d?'<div class="document-stage" id="document-stage"><img src="'+d.url+'" alt="'+esc(d.name)+'" style="'+(d.filter?'filter:grayscale(1)':'')+'"><svg id="annotation-layer" class="annotation-layer" viewBox="0 0 '+d.width+' '+d.height+'" '+(S.tool==='select'?'style="pointer-events:none"':'')+'>'+annotationMarkup(d)+'</svg></div>':emptyCanvas();
 const film=d?'<div class="filmstrip" aria-label="当前会话素材"><span class="filmstrip-label">当前素材</span>'+S.docs.map(x=>'<button class="thumb '+(x.id===S.active?'selected':'')+'" data-doc="'+x.id+'"><img src="'+x.url+'" alt=""><span>'+esc(x.name)+'<small>'+x.width+' × '+x.height+' · '+(x.result?'已有识别结果':'PNG')+'</small></span></button>').join('')+'<button class="film-add" data-action="open-image" title="添加图片" aria-label="添加图片">'+icon('plus')+'</button></div>':'';
 return '<div class="toolbar"><div class="toolbar-group">'+button('区域截图','capture','capture')+button('打开','open-image','folder')+button('长截图','scroll-capture','layers','ghost')+'<i class="separator"></i>'+button('拼图','collage','grid','ghost')+button('批处理','batch','layers','ghost')+ib('图片插件','image-plugins','spark')+'</div><div class="toolbar-group">'+(d?button('保存','save-image','download')+ib('关闭当前图片','close-document','close'):'')+'</div></div><div class="image-layout '+(!show?'no-inspector':'')+'">'+toolRail()+'<section class="canvas-area" aria-label="图片画布">'+tabs+context+'<div class="canvas" id="canvas">'+(d?rulers():'')+stage+'</div><div class="canvas-footer"><span>'+(d?d.width+' × '+d.height+' px · PNG · '+(d.annotations.length?'有未保存标注':'原始图像'):'拖放 / 粘贴 / 打开')+'</span><div class="zoom-tools">'+(d?ib('缩小','zoom-out','minus')+'<span id="zoom-value">100%</span>'+ib('放大','zoom-in','plus')+ib('适应画布','fit','fit'):'支持 PNG、JPG、WebP、GIF')+'</div></div>'+film+'</section>'+(show?recognitionPanel(d):'')+'</div>';
};
recognitionPanel=function(d){
 if(!d)return '<aside class="inspector">'+inspectorTabs()+'</aside>';
 if(S.inspectorTab==='info')return '<aside class="inspector">'+inspectorTabs()+'<section class="panel-section"><div class="section-title">当前图像</div><dl class="info-grid"><dt>名称</dt><dd>'+esc(d.name)+'</dd><dt>尺寸</dt><dd>'+d.width+' × '+d.height+' px</dd><dt>格式</dt><dd>图像预览</dd><dt>标注</dt><dd>'+d.annotations.length+' 处</dd><dt>识别结果</dt><dd>'+(d.result?'已保留':'无')+'</dd></dl></section>'+button('保存图像','save-image','download')+button('贴到桌面','pin','pin')+'<p class="privacy-note">原型素材保留在本次预览会话中。</p></aside>';
 const running=S.jobs.find(j=>j.docId===d.id&&j.status==='running');
 return '<aside class="inspector">'+inspectorTabs()+'<section class="panel-section"><div class="section-title">识别方式'+ib('识图服务设置','service-settings','settings')+'</div><div class="segmented" role="tablist" aria-label="识别方式"><button data-mode="ocr" role="tab" aria-selected="'+(S.mode==='ocr')+'" class="'+(S.mode==='ocr'?'selected':'')+'">文字 OCR</button><button data-mode="vision" role="tab" aria-selected="'+(S.mode==='vision')+'" class="'+(S.mode==='vision'?'selected':'')+'">'+icon('spark')+' 多模态</button></div>'+(S.mode==='ocr'?'<div class="field-row"><label for="ocr-language">识别语言</label><select id="ocr-language"><option>自动检测</option><option>简体中文 + English</option><option>English</option></select></div><div class="panel-note">离线识别 · 在本机处理</div>':'<div class="task-chips">'+[['text','提取文字'],['table','提取表格'],['describe','描述画面'],['question','自由提问']].map(([k,n])=>'<button class="chip '+(S.task===k?'selected':'')+'" data-task="'+k+'">'+n+'</button>').join('')+'</div><details '+(S.task==='question'?'open':'')+'><summary>补充要求（可选）</summary><div class="field"><textarea id="recognition-prompt" placeholder="输入要求或问题">'+esc(d.prompt||'')+'</textarea></div></details><div class="panel-note">在线识图 · 使用已配置的服务</div>')+(running?'<div class="job-inline"><span class="spinner"></span>正在识别…</div>'+button('取消识别','cancel-ocr','','full'):'<button class="button primary full" data-action="recognize" '+(enabled(S.mode)?'':'disabled')+'>'+icon(S.mode==='ocr'?'text':'spark')+'开始识别</button>'+(enabled(S.mode)?'':'<p class="panel-note">此功能已停用，请在插件中启用。</p>'))+'</section><section class="result-block"><div class="section-title">识别结果<span class="subtle">'+(d.result?'可直接校对':'等待识别')+'</span></div><textarea id="ocr-result" class="result-editor" aria-label="识别结果，可直接校对" placeholder="识别结果显示在这里">'+esc(d.result||'')+'</textarea><div class="result-actions"><span>'+(d.result?d.result.length+' 字 · 示例结果':'随当前图像保留')+'</span>'+button('复制','copy-result','copy','small')+'</div></section><p class="privacy-note">'+icon('shield')+(S.mode==='ocr'?'当前图片在本机处理。':'主动识别时，图片将发送到配置的服务。')+'</p></aside>';
};
annotationMarkup=function(d){return d.annotations.map(a=>a.kind==='rect'?'<rect x="'+a.x+'" y="'+a.y+'" width="'+a.w+'" height="'+a.h+'" fill="none" stroke="#b87c53" stroke-width="3" rx="1"/>':'<path d="'+a.path+'" fill="none" stroke="#b87c53" stroke-width="3" stroke-linecap="round"/>').join('')};
const documentResult='【演示识别结果】\n区域截图与识别工作流\n\n01 工作流约定\n截图完成后，图片回到当前工作区。用户可以继续标注、裁剪，或在右侧选择文字识别；切换功能时保留当前内容。\n\n截取 / 导入图片\n→ 标注与识别\n→ 校对 / 复制 / 保存\n\n02 待确认事项\n✓ 识别结果与原图一一对应\n✓ 离线 OCR 与多模态在同一面板切换\n□ 快捷键和高 DPI 布局检查';
recognize=function(){
 const d=doc();if(!d||!enabled(S.mode)||S.jobs.some(j=>j.docId===d.id&&j.status==='running'))return;
 const job={id:Date.now(),docId:d.id,name:'识别 '+d.name,status:'running',mode:S.mode,task:S.task};S.jobs.push(job);render();status('正在识别 '+d.name);
 job.timer=setTimeout(()=>{if(job.status!=='running')return;job.status='done';d.result=job.mode==='vision'&&job.task==='table'?'【演示识别结果】\n事项\t状态\n原图与结果关联\t已确认\n切换时保留内容\t已确认\n高 DPI 布局检查\t待确认':job.mode==='vision'&&job.task!=='text'?'【演示识别结果】\n这是一份产品评审记录，包含截图、标注、识别及素材保存的工作流，并列出了三项待确认事项。':documentResult;render();toast('演示识别完成，结果已保留在对应图像');},1800);
};
const legacyVideo=videoPage,legacyEditor=videoEditor;
videoPage=function(){
 if(S.record.editing)return videoEditor();
 let html=legacyVideo();
 const start=html.indexOf('<div class="record-grid">');
 html=html.slice(start).replace(/<\/div>$/,'').replace('产品操作演示.panzo','工作流录制.panzo').replace('自动保存','录制参数').replace('Panzo Studio · 应用窗口','已选择窗口');
 return '<div class="toolbar"><div class="toolbar-group"><span class="tool-caption">屏幕录制</span>'+button('选择区域','choose-region','capture')+'</div><div class="toolbar-group">'+button('打开工程','open-video','folder')+'</div></div>'+html;
};
videoEditor=function(){return legacyEditor().replaceAll('产品操作演示','工作流录制').replace('雾蓝','灰绿').replace('画面与镜头</div>','画面与镜头</div>')};
libraryPage=function(){
 const assets=allAssets().filter(a=>(S.assetsFilter==='all'||a.type===S.assetsFilter)&&a.name.toLowerCase().includes(S.assetQuery.toLowerCase()));
 return '<div class="toolbar"><div class="toolbar-group">'+button('导入素材','open-image','folder')+'<span class="tool-caption">本地素材 / '+({all:'全部文件',image:'图片',video:'视频工程',gif:'GIF'}[S.assetsFilter])+'</span></div><div class="search-field">'+icon('search')+'<input id="asset-search" class="input" placeholder="搜索素材" aria-label="搜索素材" value="'+esc(S.assetQuery)+'"></div></div><div class="library-layout"><aside class="library-folders"><h3>素材类型</h3>'+[['all','全部文件','folder'],['image','图片','image'],['video','视频工程','video'],['gif','GIF','layers']].map(([k,n,i])=>'<button data-filter="'+k+'" class="'+(k===S.assetsFilter?'selected':'')+'">'+icon(i)+n+'<small>'+allAssets().filter(a=>k==='all'||a.type===k).length+'</small></button>').join('')+'<div class="folder-foot">'+icon('shield')+' 本地素材 · 示例数据</div></aside><div class="library-content"><div class="library-path">名称<span>'+assets.length+' 项 · 点击打开</span></div><div class="assets-grid" id="assets-grid">'+assetCards(assets)+'</div></div></div>';
};
pluginsPage=function(){
 const list=S.plugins.filter(p=>S.pluginFilter==='all'||p.on);
 return '<div class="toolbar"><div class="toolbar-group">'+button('安装本地插件','install-plugin','plus')+'<span class="tool-caption">'+S.plugins.length+' 个已安装</span></div><span class="tag">功能管理</span></div><div class="scroll-page"><div class="plain-tabs"><button data-plugin-filter="all" class="'+(S.pluginFilter==='all'?'selected':'')+'">全部插件</button><button data-plugin-filter="on" class="'+(S.pluginFilter==='on'?'selected':'')+'">已启用</button></div><div class="plugin-table"><div class="plugin-columns"><span></span><span>名称</span><span>说明</span><span>版本 / 来源</span><span>状态</span><span>操作</span></div>'+list.map(p=>'<article class="plugin-row"><span class="plugin-icon">'+icon(p.icon)+'</span><h3>'+esc(p.name)+'</h3><p class="plugin-desc-col">'+esc(p.desc)+'</p><span class="plugin-version">'+esc(p.version)+'</span>'+toggle(p.name,'plugin:'+p.id,p.on)+button('管理','plugin:'+p.id,'','small ghost')+'</article>').join('')+'</div><div class="plugin-description-pane"><div><h3>插件与工作区</h3><p>已启用功能显示在对应工作区；停用后保留素材与识别结果。</p><p>本地插件文件：.panzoplugin</p></div><div><h3>安装说明</h3><p>选择插件文件后，可查看版本和来源，再确认安装。</p><p>当前设计预览不执行实际安装。</p></div></div></div>';
};
const legacySettings=settingsContent;
settingsContent=function(){return legacySettings().replace('浅色 · 清晰明快','浅色').replace('深色 · 专注画面','深色').replace(/<p class="subtitle">.*?<\/p>/g,'')};
sidePanel=function(title,body,actions=''){S.panels[S.page]={title,body,actions};S.inspectorHidden=false;render()};
const legacyAction=action;
action=function(a){
 if(a.startsWith('background:')){S.videoBackground=a.slice(11);const c=$('.composition');if(c)c.style.background={blue:'#626d63',warm:'#ddd9cf',dark:'#292d2a'}[S.videoBackground];return}
 if(a==='toggle-inspector'){S.inspectorHidden=!S.inspectorHidden;render();return}
 if(a.startsWith('inspect:')){S.inspectorTab=a.slice(8);delete S.panels.image;render();return}
 if(a.startsWith('menu-')){showDesktopMenu(a.slice(5));return}
 if(a==='capture'||a==='scroll-capture'){S.inspectorHidden=false;S.inspectorTab='recognition'}
 legacyAction(a);
};
function showDesktopMenu(menu){
 const groups={file:[['打开图片','open-image','folder','Ctrl O'],['区域截图','capture','capture',''],['保存图片','save-image','download',''],['关闭当前图片','close-document','close','']],edit:[['撤销标注','undo','undo','Ctrl Z'],['拼图','collage','grid',''],['批处理','batch','layers','']],view:[['适应画布','fit','fit',''],['显示 / 隐藏识别面板','toggle-inspector','layers',''],['搜索命令','desktop-search','search','Ctrl K']]};
 const old=$('#desktop-menu');if(old){old.remove();return}
 const el=document.createElement('div');el.id='desktop-menu';el.className='desktop-menu';el.setAttribute('role','menu');el.innerHTML=groups[menu].map(([n,a,i,k])=>'<button role="menuitem" data-desktop-command="'+a+'">'+icon(i)+'<span>'+n+'</span><kbd>'+k+'</kbd></button>').join('');
 const b=$('[data-action="menu-'+menu+'"]'),r=b.getBoundingClientRect();el.style.left=r.left+'px';el.style.top=r.bottom+'px';document.body.append(el);
}
document.addEventListener('click',e=>{const b=e.target.closest('[data-desktop-command]');if(b){$('#desktop-menu')?.remove();if(b.dataset.desktopCommand==='desktop-search')showCommands();else action(b.dataset.desktopCommand)}else if(!e.target.closest('#desktop-menu')&&!e.target.closest('[data-action^="menu-"]'))$('#desktop-menu')?.remove()});
document.addEventListener('keydown',e=>{if(e.key==='Escape')$('#desktop-menu')?.remove()});
// Host hint only: a normal browser tab / --app shell may keep its own caption.
// The native Panzo host must apply these same chrome/text tokens through DWM.
function syncWindowThemeHint(){
 const color=getComputedStyle(document.documentElement).getPropertyValue('--chrome').trim();
 document.querySelector('meta[name="theme-color"]').content=color;
 document.querySelector('meta[name="color-scheme"]').content=S.theme;
}
const legacyRender=render;
render=function(){legacyRender();syncWindowThemeHint();$('#window-document').textContent=S.page==='image'?(doc()?.name||'未打开文档'):pages[S.page][1];$('#footer-hint').textContent=S.page==='image'?'Ctrl O 打开  ·  Ctrl V 粘贴':'工作区切换保留当前状态';$('#desktop-menu')?.remove()};
showHelp=function(){modal('Panzo · 桌面工作台设计','<p>这是新版桌面布局的交互设计预览，原生应用尚未替换界面。</p><ul><li>顶部切换工作区，文档标签与底部缩略图切换图像。</li><li>左侧为编辑工具，右侧识别面板可收起。</li><li>可以打开、拖放本地图片，试用矩形 / 画笔与撤销。</li><li>录制、识别、导出和插件安装均为流程演示。</li><li>只保存主题偏好；密钥不会保存，也不连接外部服务。</li></ul>',button('关闭','close-modal','','primary'))};
$('#help').onclick=showHelp;$('#tour').onclick=showHelp;
if(!new URLSearchParams(location.search).has('empty')){
 const a=addDoc(sampleImage(1),'交互检查表.png');
 const b=addDoc(sampleImage(),'区域截图_01.png');
 b.result=documentResult;
 b.annotations.push({kind:'rect',x:57,y:235,w:697,h:59});
 render();
}else render();
