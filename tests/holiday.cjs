const assert = require('node:assert/strict');
const fs = require('node:fs/promises');
const path = require('node:path');
const http = require('node:http');
const {chromium} = require('playwright');
(async () => {
  const root = path.resolve(__dirname, '../ui');
  const server = http.createServer(async (req, res) => {
    const file = path.resolve(root, req.url.split('?')[0].replace(/^\/+/, '') || 'index.html');
    if (!file.startsWith(root + path.sep)) return res.writeHead(403).end();
    try { res.setHeader('Content-Type', {'.html':'text/html', '.js':'text/javascript', '.css':'text/css'}[path.extname(file)] || 'application/octet-stream'); res.end(await fs.readFile(file)); }
    catch { res.writeHead(404).end(); }
  });
  await new Promise(r => server.listen(0, '127.0.0.1', r));
  let browser;
  try {
    browser = await chromium.launch({headless:true, executablePath:process.env.CGS_BROWSER});
    const page = await browser.newPage({viewport:{width:1500,height:1000}});
    const errors = [];
    page.on('pageerror', e => errors.push(e.message));
    await page.route('https://**/*', r => r.abort());
    await page.addInitScript(() => {
      localStorage.setItem('cgs-language', 'en');
      window.__TAURI__ = {event:{listen:async()=>()=>{}},core:{invoke:async name => {
        if(name==='platform_info')return{os:'windows',windows:true,windowsOnlyModules:[]};
        if(name==='list_games')return{games:[],selected_id:null};
        if(name==='get_app_status')return{games_count:0,admin:false,game_running:false};
        if(name==='check_for_update')return{available:false,currentVersion:'5.6.4',latestVersion:'5.6.4'};
        if(name==='local_cloud_status')return{enabled:false,eligibleApps:[]};
        if(name==='files_catalog')return{files:[]};
        if(name==='scan_installed_steam_games')return{games:[],libraries:[],warnings:[]};
        return{};
      }}};
    });
    await page.goto(`http://127.0.0.1:${server.address().port}/`);
    await page.waitForFunction(()=>window.cgsPersonalization && document.querySelector('#mascot-pet svg'));
    assert.equal(await page.locator('html').getAttribute('data-ui-holiday'), 'on');
    assert.match(await page.locator('.holiday-season').innerText(), /Winter edition/i);
    await page.waitForFunction(()=>{
      const c=document.querySelector('.holiday-snow');
      return c.getContext('2d').getImageData(0,0,c.width,c.height).data.some((n,i)=>i%4===3&&n>0);
    });
    for (const theme of ['graphite','white','arctic']) {
      await page.evaluate(theme=>cgsPersonalization.set({theme}),theme);
      const box=await page.locator('.holiday-season').boundingBox();
      assert.ok(box.x+box.width<=1501);
      await page.screenshot({path:path.resolve(__dirname,`../build/holiday-${theme}.png`)});
    }
    await page.evaluate(()=>cgsPersonalization.set({holiday:false}));
    assert.equal(await page.locator('.holiday-season').isVisible(),false);
    await page.reload();
    assert.equal(await page.locator('html').getAttribute('data-ui-holiday'),'off');
    await page.evaluate(()=>cgsPersonalization.set({holiday:true,motion:false}));
    assert.equal(await page.locator('.holiday-snow').isVisible(),false);
    assert.equal(await page.locator('.holiday-season').isVisible(),true);
    await page.evaluate(()=>cgsPersonalization.set({motion:true,scale:125,sidebar:'icons'}));
    await page.emulateMedia({reducedMotion:'reduce'});
    assert.equal(await page.locator('.holiday-snow').isVisible(),false);
    await page.evaluate(()=>cgsI18n.setLanguage('ru'));
    assert.match(await page.locator('.holiday-season').innerText(),/Новогодний сезон/i);
    assert.deepEqual(errors,[]);
    console.log('Holiday: themes, snow, persistence, scale, language and reduced motion passed');
  } finally { await browser?.close(); server.close(); }
})().catch(e=>{console.error(e);process.exitCode=1;});
