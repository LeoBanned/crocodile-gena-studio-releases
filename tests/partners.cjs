const assert=require('node:assert/strict');
const fs=require('node:fs/promises');
const path=require('node:path');
const http=require('node:http');
const {chromium}=require('playwright');
(async()=>{
  const root=path.resolve(__dirname,'../ui');
  const seed=JSON.parse(await fs.readFile(path.join(__dirname,'../src-tauri/src/assets/partners.json'),'utf8'));
  const server=http.createServer(async(req,res)=>{
    const file=path.resolve(root,decodeURIComponent(req.url.split('?')[0]).replace(/^\/+/, '')||'index.html');
    if(!file.startsWith(root+path.sep))return res.writeHead(403).end();
    try{res.setHeader('Content-Type',{'.html':'text/html','.css':'text/css','.js':'text/javascript','.png':'image/png'}[path.extname(file)]||'application/octet-stream');res.end(await fs.readFile(file));}catch{res.writeHead(404).end();}
  });
  await new Promise(r=>server.listen(0,'127.0.0.1',r));
  let browser;
  try {
    browser=await chromium.launch({executablePath:process.env.CGS_BROWSER||'C:/Program Files (x86)/Microsoft/Edge/Application/msedge.exe',headless:true});
    const page=await browser.newPage({viewport:{width:1600,height:1100},reducedMotion:'reduce'});
    const errors=[];page.on('pageerror',e=>errors.push(e.message));
    await page.route('https://**/*',r=>r.abort());
    await page.addInitScript(seed=>{
      window.partnerData={partners:seed.partners,source:'gist',stale:false};window.partnerCalls=[];
      window.__TAURI__={event:{listen:async()=>()=>{}},core:{invoke:async(name,args)=>{
        window.partnerCalls.push({name,args});
        if(name==='partners_catalog')return structuredClone(window.partnerData);
        if(name==='platform_info')return{os:'windows',windows:true,windowsOnlyModules:[]};
        if(name==='check_for_update')return{available:false,currentVersion:'5.6.4',latestVersion:'5.6.4'};
        if(name==='list_games')return{games:[],selected_id:null};
        if(name==='get_app_status')return{games_count:0,admin:false,game_running:false};
        if(name==='is_selected_game_running')return false;
        if(name==='local_cloud_status')return{enabled:false,managed:false,eligibleApps:[],loaderSupported:true};
        if(name==='luatools_status')return{steamPath:'C:\\Steam',games:[],loaderStatus:'ready'};
        if(name==='luatools_check_loader_updates')return{updateAvailable:false,selectedVariant:'standard',variants:[]};
        if(name==='millennium_plugin_status')return{status:'ready',pluginInstalled:true,millenniumInstalled:true};
        if(name==='files_catalog')return{files:[],cached:false};
        if(name==='scan_installed_steam_games')return{games:[],libraries:[],warnings:[]};
        return{};
      }}};
    },seed);
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.locator('[data-first-language="en"]').click();
    assert.equal(await page.evaluate(()=>window.cgsI18n.language),'en');
    assert.equal(await page.locator('.social-links button').count(),1);
    await page.locator('#open-social-hub').click();
    const social=page.locator('#social-hub');
    await social.waitFor({state:'visible'});
    assert.equal(await social.locator('[data-social]').count(),3);
    assert.deepEqual(await social.locator('.author').allTextContents(),['Author','Author']);
    assert.deepEqual(await social.locator('.community').allTextContents(),['Community']);
    const box=await social.boundingBox();
    assert(Math.abs(box.x+box.width/2-800)<2 && Math.abs(box.y+box.height/2-550)<2,'Hub must be centered');
    for(const key of ['author-telegram','community-telegram','author-discord']) await social.locator(`[data-social="${key}"]`).click();
    assert.deepEqual(await page.evaluate(()=>partnerCalls.filter(c=>c.name==='open_external').map(c=>c.args.url)),[
      'https://t.me/crocodilehub2','https://t.me/piratestationn','https://discord.gg/Vwp5DAnFG'
    ]);
    for(let i=0;i<6;i++)await page.keyboard.press('Tab');
    assert(await page.evaluate(()=>document.getElementById('social-hub').contains(document.activeElement)));
    for(const theme of ['graphite','white','ocean']){
      await page.evaluate(theme=>window.cgsPersonalization.set({theme}),theme);
      await social.screenshot({path:path.join(process.env.CGS_TEST_OUTPUT||require('node:os').tmpdir(),`social-hub-${theme}-en.png`)});
    }
    await page.keyboard.press('Escape');
    assert.equal(await social.isVisible(),false);
    assert.equal(await page.evaluate(()=>document.activeElement.id),'open-social-hub');
    await page.evaluate(()=>window.cgsPersonalization.set({sidebar:'icons'}));
    await page.locator('#open-social-hub').click();await social.waitFor({state:'visible'});
    await page.locator('#social-hub-close').click();
    await page.evaluate(()=>window.cgsPersonalization.set({sidebar:'full'}));
    await page.locator('.nav-item[data-view="partners"]').click();
    await page.locator('.partner-spotlight').waitFor();
    await page.waitForFunction(()=>!document.getElementById('partners-refresh').disabled);
    assert.equal(await page.locator('.partner-name').textContent(),'XERO NATION');
    assert.equal(await page.locator('.partner-features li').count(),4);
    await page.locator('.partner-join').click();
    assert.equal(await page.evaluate(()=>partnerCalls.filter(c=>c.name==='open_external').at(-1).args.url),'https://discord.gg/nationxero');
    for(const theme of ['graphite','white','ocean','forest']){
      await page.evaluate(theme=>window.cgsPersonalization.set({theme}),theme);
      await page.locator('#view-partners').screenshot({path:path.join(process.env.CGS_TEST_OUTPUT||require('node:os').tmpdir(),`partners-${theme}-en.png`)});
    }
    for(const language of ['ru','en']){
      await page.evaluate(language=>window.cgsI18n.setLanguage(language),language);
      for(const width of [1080,1280,1600]){
        await page.setViewportSize({width,height:900});
        assert(await page.locator('#view-partners').evaluate(e=>e.scrollWidth<=e.clientWidth+1),`${language} overflow at ${width}`);
      }
    }
    await page.evaluate(()=>{
      const first=partnerData.partners[0];
      partnerData.partners.push({...first,id:'new-partner',name:'A NEW PARTNER',featured:false,bannerUrl:'',logoUrl:'',links:[{label:'Visit',url:'https://example.com'}]});
    });
    await page.locator('#partners-refresh').click();
    await page.waitForFunction(()=>document.querySelectorAll('[data-partner-id]').length===2);
    assert.equal(await page.locator('.partner-card .partner-name').textContent(),'A NEW PARTNER');
    await page.evaluate(()=>{partnerData.stale=true;partnerData.source='cache';});
    await page.locator('#partners-refresh').click();
    await page.waitForFunction(()=>document.getElementById('partners-status').textContent.includes('saved catalog'));
    assert.equal(await page.locator('[data-partner-id]').count(),2);
    await page.evaluate(()=>{partnerData.partners[1].name='<img src=x onerror="window.injected=true">';partnerData.partners[1].links[0].url='javascript:alert(1)';});
    await page.locator('#partners-refresh').click();
    await page.waitForFunction(()=>document.querySelector('.partner-card .partner-name').textContent.includes('<img'));
    assert.equal(await page.evaluate(()=>!!window.injected),false);
    assert.equal(await page.locator('.partner-card button').count(),0);
    await page.evaluate(()=>partnerData.partners=[]);
    await page.locator('#partners-refresh').click();
    await page.locator('.partners-empty').waitFor();
    assert.equal(await page.locator('[data-partner-id]').count(),0);
    assert.deepEqual(errors,[]);
    await page.evaluate(()=>window.cgsI18n.setLanguage('ru'));
    await page.reload();
    assert.equal(await page.evaluate(()=>window.cgsI18n.language),'ru');
    await page.setViewportSize({width:1080,height:700});
    await page.locator('#open-social-hub').click();await social.waitFor({state:'visible'});
    assert.deepEqual(await social.locator('.author').allTextContents(),['Автор','Автор']);
    assert.deepEqual(await social.locator('.community').allTextContents(),['Комьюнити']);
    const smallBox=await social.boundingBox();assert(smallBox.y>=0 && smallBox.y+smallBox.height<=700);
    console.log('Social hub: single sidebar entry, 3 correct links, Author/Community badges, centering, focus/Escape, compact sidebar, themes, English default and saved Russian passed.');
    console.log('Partners UI: external link, dynamic addition/removal, offline cache, 4 themes, RU/EN, 3 viewport widths and untrusted content passed. Mock IPC.');
  } finally {await browser?.close();await new Promise(r=>server.close(r));}
})().catch(e=>{console.error(e);process.exitCode=1;});
