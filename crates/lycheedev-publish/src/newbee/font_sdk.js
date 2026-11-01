const fs = require('node:fs');
const input = JSON.parse(fs.readFileSync(0, 'utf8'));
let started = false;
const send = (value, status) => process.stdout.write(
  '\nLYCHEE_FONT_RESULT:' + JSON.stringify({schema: 'lycheedev.publish.font-sdk.v1', ...value}) + '\n',
  () => process.exit(status),
);
(async () => {
  try {
    const core = require(input.binding);
    if (typeof core.OssUploader !== 'function') throw new Error('missing uploader');
    if (typeof core.setLogCallback === 'function') core.setLogCallback(() => {});
    const uploader = new core.OssUploader(JSON.stringify({apiBase: 'https://api.next.newbeebox.com/uploadserver'}));
    if (typeof uploader.uploadFolder !== 'function') throw new Error('missing folder capability');
    started = true;
    const result = await uploader.uploadFolder(JSON.stringify({path: input.path}));
    if (!result || result.code !== 1 || typeof result.data !== 'string' || !/^[A-Za-z0-9_-]{1,256}$/.test(result.data)) {
      send({ok: false, started}, 1); return;
    }
    send({ok: true, started, index: result.data}, 0);
  } catch (_) { send({ok: false, started}, 1); }
})();
