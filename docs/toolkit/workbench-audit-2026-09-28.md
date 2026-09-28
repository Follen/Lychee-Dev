# 工作台审计与统一修订 — 2026-09-28

范围：WoW Frame API 工作台八页，共享 Widgets/Theme 与外壳。按用户截图和
相邻 Lychee 的 Components/ResultList 核对，设计统一维护在根目录 `DESIGN.md`。
这是原生游戏 UI，网页 DOM、ARIA 和移动端断点不适用；未做屏幕阅读器认证。

| 问题 | 根因 | 修订 |
| --- | --- | --- |
| P1 红色“已确认” | 收取阶段文字与执行失败颜色混用 | 主状态显示业务结果，详情保留收取阶段 |
| P2 运行页历史像空洞 | 历史容器和窗口同色，顶边不齐 | 改为字段底面，与编辑区对齐 |
| P2 搜索/刷新/清理像标签 | 次级操作按钮透明，仅主动作有底 | 共用中性按钮底面和尺寸，主次由文字表达 |
| P2 诊断摘要蓝灰 | 页面硬编码 .55/.60/.65 | 统一 textMuted，错误才用 danger |
| P2 自动化行难区分 | 容器和选中底同色；第一版独立黑条又过重 | 透明行、4 单位间隔、低对比圆角选中底和短红线 |
| P2 滚动条过重 | 灰轨及接近整屏的宽拇指 | 参考 Lychee：3 宽红滑块、24–48 高、无底轨 |
| P2 已废弃的通知操作 | 原生内存通道仍构造旧 QR 操作按钮 | 原生页面不再构造两个按钮 |
| P3 冗余说明及设置页 | 只读快捷键和无关状态占据界面 | 删除设置页/入口及关于页战斗保护说明 |
| P1 打开页面后 Agent 收尾卡住 | 运行/对象/追踪的 activate 自动 SetFocus，编辑框吞掉确认键 | 移除页面激活的自动抢焦点，保留主动点击编辑 |

底层战斗检查、固定快捷键、任务收尾和已有 reduced-motion 偏好保留。
没有借视觉修订引入新的调查操作、自动执行或全局输入拦截。

验证入口：`tests/channel-live/workbench-visual.mjs`。使用显式项目、安装、PID，
打开八页并分别 WGC 捕获；after 模式断言设置/通知入口已移除、失败行文案正确。
最后释放准确的 CON；失败保留连接与原始报告，不自动 abandon。
源码行为检查与真实视觉判断分别记录，不以截图脚本退出成功代替目视验收。

## 本轮证据

- `go build ./...`、`go vet ./...` 通过；强制 Lua 5.1 的
  `go test -count=1 ./...` 通过，原始输出为
  `.tmp/channel-live/workbench-final-go3.txt`。版本/Skill 合同检查通过。
- 私有候选 `.tmp/channel-live/package-workbench-ready/dev-npm-stage` 已受管安装
  并 reload 激活到 Retail **12.1.0.69933**，PID 31208，灵止光—死亡之翼。
  缺失根目录 `version.txt` 时没有推断 build；以上身份来自原生协议发现与连接校验。
- `.tmp/channel-live/retail-workbench-after/workbench.json`：19 步通过，八页分别
  保留 WGC 图片，九次页面操作均完成精确收尾；CON-8a422b783a58ea80a6c75e6848c23e4c
  已确认断开。八页截图已目视检查：布局无截断、历史底面完整、按钮尺寸统一、
  诊断摘要为暖灰、自动化条目分开、关于页不再显示战斗保护、侧栏没有设置。
- `.tmp/channel-live/retail-workbench-history/automation-history.json`：9 步通过。
  成功/预期失败记录跨 reload 的 ID、报告字节和角色一致；失败行显示“执行失败”，
  详情保留“已确认”，禁止重放。12/13 条记录时滚动范围约 338/408，原生 Slider
  到达首尾、3 宽/24–48 高滑块断言通过；reload 后 WGC 已目视核对选中行与红滑块。
  本次还实际发生一次槽位容量换代 reload，保留记录未丢失。
  CON-e6e324ff5ef4e5765776cc5fd8149ecb 已确认断开，正式服没有遗留本轮测试连接。

改前证据包含用户截图和 `.tmp/channel-live/workbench-before-final/runner/` WGC。
Classic 改前流程在对象页暴露自动聚焦吞掉确认键的问题，随后原 PID 28276 退出。
CON-8c57f45bd295970b55aaf7582b85da83 的 LMO-3f62a09037344c4813e51bdd6166aaa4
仍保留为 `confirm_ready`、报告未核验、收尾未完成，未转移到正式服、未伪造 ACK。
第一条改前连接在有限只读确认恢复后已断开；上述第二条原始失败报告保持不变。
用户随后明确选择正式服完成改后检查。新版 Classic、英文及真实多缩放仍为 `not_run`。

## 输入状态恢复与最终列表修订

- Native 固定 Ctrl+Alt+F12。实测单换非字符按键仍会被有焦点的 EditBox 吞掉，
  原失败 `retail-f12-focus/focus.json` 保留；不能把 F12 宣称为绕过所有焦点。
- 按用户决定，主插件通过 kind 5 内存记录发布 `inputBlocked`。CLI 校验 runtime、
  actor/build、owner/fence、nextSlot 及采样时效；普通焦点阻挡才逐个发 Esc，随后重新
  观察。无新增色块，无前台 SendInput，无第三方编辑框 hook/ClearFocus。
  Esc 会按原界面的处理器关闭或取消界面，这是用户明确接受的行为。
- `.tmp/channel-live/retail-inputblock-focus/focus.json` 六步通过：单行和多行各收到
  精确三次 Esc，随后完成原槽位，文本/光标不变、fixture watchdog 未超时。
  CON-ac20939b7a723d025a5addf2f4c4eb5f 已断开。日志记录的是发送前意图，
  陈旧采样可使意图在实际发键前终止，因此意图计数不等于游戏接收次数。
- 重连暴露两项采样问题：扫描优先区域时新采样尚未生成；大量旧采样消耗原始命中
  上限。现先等待采样窗口，扫描器以流式 visitor 验证候选，拒绝的旧记录不计入
  有效结果上限，找到有效记录立即停止匹配。总字节、deadline、有效结果数继续有界；
  提前结束不宣称完整覆盖。新增 `TestLookupStreamsPastRejectedOldTelemetry`。
  原失败 CON-f27ced4b1ccf6b786ecf3f37d2a850ac 沿同一日志恢复并正常断开。
- 后续页面检查遇到系统按键已按住：原始 `events.json` 与 input_attempted 日志保留。
  未来遇到这一情况，只有底层证明零条按键消息入队才落盘 `input_not_sent`，保留同一
  nonce/槽位等待重试；部分/未知发送仍禁止重放。对应离线恢复测试已加入。
  原事件检查通过受控 reload 后以 observation 策略恢复，同一操作 ID 完成，
  证据为 `retail-inputblock-ui-verified/events-recovered.json`，没有篡改旧失败报告。
- 原生 disconnect 与其他命令共用 deadline 内的等待逻辑，pending 输入状态不再
  只试一次就结束。该变化不允许重发已尝试且结果未知的 unbind。

上述新输入行为只在 Retail 12.1.0.69933 实测；Classic/Titan/Forever、任意第三方
键盘拦截器、真实战斗及 secret 焦点边界保持 `not_run`，离线覆盖不替代实机。

本批最终收尾：`automation-final.json` 报告通过，8 个可见列表行均使用新的圆角选择层，
`focusFree=true`。`automation-final-view/evidence/20260928T042153.164028400-capture/frame.png`
已通过 WGC 目视核对：无逐行黑条，选中态较淡，保留短红线和细红滑块。
`disconnect-final.json` 确认 CON-d360bda672375add3012e2beeb37474d 已关闭。
本次八页脚本中途失败的原始 workbench.json 保留，不改成整组通过。
强制 Lua 5.1 全量测试最终通过：`.tmp/channel-live/inputblock-accepted-go.txt`；
build、vet、Skill 合同、版本一致性检查通过。没有发布新 npm 或提交 git。

用户随后要求先审架构，新增功能实现暂停在此。下一步按
[控制架构复审](live-input-architecture-2026-09-28.md) 收敛，不能将本批局部验证
表述为控制架构已完成重构或全客户端验收。
