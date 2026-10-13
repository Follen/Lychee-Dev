# Native 两槽比较候选（2026-10-08）

状态：准备比较，不是定稿。四槽 exact COMMIT 保持原稿与受测源码；其机制是可验证对照，不是技术下限或既定最优。下一受控窗口先执行已经准备的四槽 Rust 原型，再进入本稿的有限增量；不因两槽目标丢弃首次失败或放宽 B1、冷启动、私钥边界、500槽、业务等价及最终实机要求。

## 从固定源码能证明什么

使用当前 Source 已固定的四份官方 UI 源码，commit 为 Retail `31c7f7b9cc79e56c986b365c06a6afbcf3c9177b`、Classic `a53b9b28857fd11184573f8dda6a746a789abc0e`、Titan `0f73fd6b0e12bf8de89a8084ebd81aade0afe8d7`、Forever `15666a6e67938a1ab5caf041406464251db111ca`。从现有只读mirror提取20份原blob，每份核对Git blob SHA-1和原始字节SHA-256后封存，没有fetch或运行客户端。

证据：`.tmp/rust-supervision/two-slot-pinned-primitives-ba19395c-a1f1-4051-b1c0-fe5d32a6d341/`，提取器 `protocol/native-two-slot-comparison/pinned-primitives.mjs`。`pins.json`、完整原源码、函数声明行号、blob身份和实际Git调用均保存。

| 原语 | 固定源码事实 | 仍不能推导 |
| --- | --- | --- |
| `GetTimePreciseSec` | 四pin的`OsDocumentation.lua`均声明返回number | 单调不回退、跨进程/系统暂停前进、与host QPC映射及误差/漂移/重置上限。声明存在不是这些强性质的证明 |
| `GetTime`等clock | `SystemTimeDocumentation.lua`声明部分候选；现有Compat仅包装GetTime | 不保证某个clock在所有客户端都有相同native来源。不能以GetTimePreciseSec名字替代实现证明 |
| `debugprofilestop` | 四pin明确是距最近一次`debugprofilestart`的毫秒 | 另一调用可重置其基准；不得无证据作为不可回退的runtime授权clock |
| EditBox focus/byte limit | 四pin均声明SetMaxBytes、SetFocus、ClearFocus、HasFocus | 实际输入输送、焦点丢失/获取、protected/secret及各客户端行为。Retail/Forever文档含ForbiddenAspect.ScriptedInput/QueryFocus及SecretAspect.Text |
| Frame keyboard | 四pin的EnableKeyboard标protected，SetPropagateKeyboardInput有restrictions | 不能在战斗中假定调用合法，不能默认不截走聊天或玩法按键 |
| `bit` | 固定源码存在band等调用，Retail也存在bxor调用 | 这不是全部客户端实际函数存在、unsigned高位语义、可信closure隔离或强校验吞吐证明。P1须函数/向量/性能实测 |

因此当前没有可直接批准的跨暂停expiry原语。此前 frozen-clock 反例保持有效的模型警示；后续证明可来自独立static/native依据加最终客户端验收，不能先假定一个函数名字满足要求。

## 路线1：授权后只发布RUN或RECEIVED一个文件

每个独立任务只有RUN一槽、最终RECEIVED一槽。主机先获得输入资格并保存完整授权，才发布不可变exact业务文件；旧key仅促使读取已经授权的exact文件。私有入口还要复核身份、当前输入事实、expiry和序号；结果收妥仍以host durable结果为前提。

单靠“MAC有效＋年龄≤Δ”目前不能继承P0本次nonce性质。Root独立反例已经实际执行，见 `rust-native-two-slot-age-model-2026-10-08.md`；本稿设计者已读完整源码/结果，未重复执行。即使给予精确单调跨暂停clock映射误差0，Δ内旧READY仍可在私有combat已变blocked后被重放，错误满足host资格；私有游戏入口拒绝不能倒算host未被错误授予输入资格。反过来，P0本次nonce也不证明采样后的输入条件永不变化，读→发送仍是B1。

可比较两种显式补足，均不是已经成立的产品机制：

1. **认证时间下界的只观察入口。** 一个不消费LoD、不能执行业务/收妥/取消的OBSERVE按钮，读取私有当前事实并签名publish。在有独立可信clock映射的前提下，host除age上界外要求publish确定晚于本次intent起点；不能仅以“刚按OBSERVE”推导响应来自该键，更不能拿旧记录建立clock映射再用这个映射证明旧记录新鲜。这能利用明确的新时间信息，但误差区间和首次clock映射仍须证明，且不关闭采样后条件变化/B1。
2. **本次nonce只观察输入。** 不消费LoD的有界输入入口接收新nonce和完整intent digest，私有只观察closure输出认证本次回答。该请求本身可作为明确非业务例外；它绝不能执行RUN、改变owner、取消或释放BODY。公开nonce回显不是认证，返回仍需私有MAC。新nonce必须通过真实物理载体，不能由模型直接写进receiver；因此它引入文本/符号、焦点、部分消息和释放成本，不能隐藏成“零额外消息”。

第一项若只承诺“读过年龄合格的旧事实、游戏决定接受”，须由完整合同和B1决定审查，不能自行降低要求。第二项的强观察关联复用P0信息，但独立观察帧要逐命令发生；每个任务的RUN和RECEIVED都计入。周期采样不是免费优化：开启时频率、HMAC/分配、事件/定时器寿命及禁用态成本必须明确测量。

## 路线2：每逻辑命令一个STAGE文件，实际提交携带认证exact token

每个逻辑命令提前发布一个只准备、不执行业务的LoD；STAGE callback加载完整payload并发布新nonce/digest认证观察。host通过本次gate且持久化授权后，才生成并投递COMMIT token。私有final-key入口从token自身取得完整binding/command/nonce/digest/slot/deadline及MAC，与原private pending逐项匹配；任何旧A token都不能变成B。这里不需要第二COMMIT LoD，**一任务RUN+RECEIVED共两槽**；仍有本次观察和独立提交，不是两个按键。

最小文本候选为 `LDT1:binding32:number8:nonce64:digest64:slot3:deadline8:mac64`。全部ASCII、固定字段长度、域分离HMAC-SHA256；deadline当前只用模型clock。MAC覆盖所有身份和deadline，不能仅核对公开nonce。OnTextChanged只累积/校验，完整token仍不执行业务；final Enter再次验证MAC、exact pending、private条件和expiry，先释放focus再dispatch。实际客户端可用的Hardware-key、EditBox、secret与focus事实尚未由fixture证明。

真实Lua5.1候选 `protocol/native-two-slot-comparison/token-receiver.lua` 已经过下述有限fixture执行，**Rust host和真实物理输入尚未验证**。它有STAGE→实际slot `loadfile` once-loader，以及文本/CHAR/Enter/焦点丢失/ESCAPE/战斗/clock模型回调；每个逻辑命令消耗一槽。所有正文、输入长度、命令与槽均有限；完整结果生命周期和动态500槽分配不是该文件的证明范围。

## 必须同表比较的成本

| 候选 | 每独立任务LoD | 正常物理输入成本（待实际载体核对） | 往返及常驻工作 |
| --- | --- | --- | --- |
| 四槽exact文件 | 4 | RUN、RECEIVED各一次STAGE＋一次COMMIT chord，共4组chord | 每逻辑命令本次观察＋接受/结果；无必要周期HMAC |
| 两槽passive-age文件 | 2 | 每逻辑命令一次wake，共2组chord；若要主动refresh另算 | passive状态频率、每次publish成本、clock映射/更新全部计入；age谓词反例尚未处理 |
| 两槽独立只观察＋授权文件 | 2 | 每逻辑命令OBSERVE输入及业务wake；payload观察帧另计字符、framing、final key与focus/release | 两份本次观察和两个业务提交；没有LoD不等于无RTT/CPU/事件 |
| 两槽STAGE＋认证token | 2 | RUN、RECEIVED各STAGE chord＋完整认证token＋final Enter；若独立wake/focus key也计入 | 与四槽相比少两份文件，增加所有token字符/键及UI生命周期；不预设更快/更简单 |

token精确字节/字符/keydown/up数量将由实际编码器记录，而不是只数逻辑消息。若以16符号键传同等token，比ASCII EditBox需要更多符号；若用WM_CHAR等消息，则必须验证实际客户端能接收并保持final key权限，不能把“字符串可构造”当物理载体已经存在。部分、串接、重复、迟到、focus丢失、Enter早到、非法MAC、expired、host中断和输入release要从真实回调得到拒绝事实与耗槽/资源状态。

比较包含final RECEIVED、cleanup重试、槽损耗、正常容量换代和实例协调。两槽成立后才更新方案及验收映射；若不成立，记录最小反例或具体unproven前提，不把四槽自动升为最优。

## 路线2有限Node/真实Lua51执行

2026-10-08在Root授权的轻量窗口执行 `node protocol/native-two-slot-comparison/run-token-owned.mjs`，host复验exit0，约1.67s。既有`ownedSpawnSync`外层Job拥有整个Node/Lua树，未运行Cargo、大消息crypto、任何游戏输入或部署。

外层证据：`.tmp/rust-supervision/two-slot-token-owned-2ef882f9-be31-4b4e-8bd5-a9e5dfc93ee3/`，result SHA-256 `840ccb8ef039d3f28d61105cd2865a72e4f0405fcf2ca01c9a449602fa7eb58e`。内部真实Lua轨迹：`.tmp/rust-supervision/two-slot-token-lua-f1103545-c306-4876-b5ed-918c396e48e0/`，result `03e7ff22866870226c7d1bf6e913ea871b9fc5d9f1bc157c1df1854be279d777`。前后源清单一致，inner/outer受测源码、runner和Job helper均已核对hash后copy-snapshot封存。receiver `06eb058dc6c32213db13935c06e055841e0f961a444a692a14160f8b5bcd28a3`；inner runner `5c1fa99a9e0a8a70030e75cea001a223810b123a04833be082e78a7ece8f59ac`；outer runner `7584dc7a3a6fc43d6efd01b91fdf57cface9af7f8bc0e91befa525b843711c3a`。

| 实际轨迹 | 限定结果 |
| --- | --- |
| 独立RUN及final RECEIVED | 真正加载两个LoD；两份exact认证token均经私有Lua验签、匹配、final-key后各分派一次；不是批平均或省略最后收妥 |
| 公开nonce回显但MAC=0 | `token_mac`拒绝，不能以nonce echo冒充鉴权 |
| A的旧有效认证token在B已staged后到达 | `token_exact_mismatch`，B业务未执行；真实私有签名回执绑定expected B身份 |
| 部分、串接、缺一字节、重一字节 | 四类均不执行业务；异常释放模型focus。完整合法token在文本回调仍保持inert |
| focus丢失、ESCAPE、combat | 三类均释放，旧Enter不能执行；新number/nonce/slot尝试可重新推进 |
| 模型clock前进到deadline+1 | 有效token在final-key被`private_expired`拒绝且释放；不构成真实跨暂停clock证明 |

正常两份token各为 **254 ASCII字节/字符回调**；每逻辑命令还需要final Enter一个模型回调及STAGE一个模型回调，即每独立任务508 token字节、510 CHAR+Enter模型回调、另2 STAGE回调。这不是实际OS INPUT记录数：Unicode输入通常含down/up，chord还涉及modifier和framing；实际OS输送尚未运行。内层逐CHAR回复是fixture诊断，不计作产品RTT。nonce→accepted fixture单样本RUN45.06ms、RECEIVED41.70ms包含这种IPC和小消息工作，**不是真实客户端CPU、吞吐、性能或四槽对照验收**。

outer Job证明root/tree reaped、handles/tool-identity locks closed、output readers complete、activeProcesses=0、forced=false、deadlineHit=false。这个已运行树的终态不代替所有父异常/宿主中断cut验收。第一次restricted-token失败保留在`two-slot-token-owned-93eec5d8-ee08-402b-9112-2f514083cb42`和inner`two-slot-token-lua-d23788f1-23a1-4e83-bf92-00e56a206896`，原因为固定Lua启动`sandbox spawn UNKNOWN`，未运行Lua；随后相同源码host复验通过。

这里的RECEIVED只是一份exact模型消息，**durable结果保存、BODY正确释放、cleanup失败保留/重试和取消竞争全部not_run**；没有给名字赋予未实现的收妥行为。真实Rusthost、受管安装、cold binding/private MAC边界、跨暂停expiry/B1、输入自动超时及host abort释放、最大payload成本、多实例500槽及换代仍未证明。这批通过只能支持“真实Lua可验证认证exact token与惯性文本/final-key模型入口”，不能将路线2接产品或宣布两槽完整成立。
