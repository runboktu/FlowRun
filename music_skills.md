# AI 作曲编曲提示词skill
用户输入原始歌词 + 风格(简单提示词)，经过该工作流后 1. 给歌词添加Suno tag，详细tag见suno_tag.md。2. 输出 中英对照的详细作曲提示词。详细介绍如下。
## 歌词增加Suno Tag(例子如下)
### 输入原始歌词(工作流运行时可从输入文件读取)
南方的冬
总少了一片素白
常青的树
没等来雪的覆盖
故乡的雪
从视频里飘来
那片田野
想必又成了白色的海

候鸟在雨雾中寻找航线
行李箱滑过潮湿的路面
站台广播模糊了乡音的暖
玻璃窗映着三十岁的童年

记忆将雪花 变成泛黄旧片
雪球掷出笑声 在屋檐飞远
碎成星星点点 照亮异乡无眠
那片白茫茫 再也抓不回指尖

新雪覆盖旧雪 岁岁年年
埋着纸牌蛋珠还有生锈的铁环
童年的口袋 装不下成年的思念
旅途中的我们 终将走散成虚线
列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年

新雪覆盖旧雪 岁岁年年
埋着纸牌蛋珠还有生锈的铁环
童年的口袋 装不下成年的思念
旅途中的我们 终将走散成虚线
列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年

列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年

列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年


### 输出歌词, 给歌词添加Suno tag，详细tag见suno_tag.md
[Intro]
[heavy blues,saxophone Solo]
[Verse 1]
南方的冬
总少了一片素白
常青的树
没等来雪的覆盖
故乡的雪
从视频里飘来
那片田野
想必又成了白色的海

[Verse 2]
候鸟在雨雾中寻找航线
行李箱滑过潮湿的路面
站台广播模糊了乡音的暖
玻璃窗映着三十岁的童年


[Chorus]
记忆将雪花 变成泛黄旧片
雪球掷出笑声 在屋檐飞远
碎成星星点点 照亮异乡无眠
那片白茫茫 再也抓不回指尖

[Chorus]
新雪覆盖旧雪 岁岁年年
埋着纸牌蛋珠还有生锈的铁环
童年的口袋 装不下成年的思念
旅途中的我们 终将走散成虚线
列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年

[Instrumental]
[heavy blues,saxophone Solo]


[Chorus]
新雪覆盖旧雪 岁岁年年
埋着纸牌蛋珠还有生锈的铁环
童年的口袋 装不下成年的思念
旅途中的我们 终将走散成虚线
列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年

列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年

列车穿过岁末 去向了从前
北风吹响风铃 把时光惊散
再也回不去 大雪纷飞的少年
## 歌词风格详细提示词
输出 中英对照的详细作曲提示词。风格为用户输入的风格，其余让AI 作为世界级顶级作曲、编曲家补充完成。 **注意英文歌词字符严格限制在1000字符以内，同时为了不必要的字符，如换行，过多空格占用字符，可定义tool删除无用字符，最终输出如下**
### 中文
人声： 低沉沙哑、饱经风霜的嗓音 – 蓝调 / R&B / 放克融合，情绪化的深夜酒吧氛围：克制、富有律动、忧郁却又不失节奏感
鼓： 主歌部分使用刷奏军鼓与鼓边击；副歌部分使用紧凑的背拍
贝斯： 主歌部分使用放克击勾弦技巧；副歌部分使用行走贝斯
沙锤/铃鼓： 贯穿全曲
键盘： 使用带有延伸和弦（九度、十三度）及蓝调音簇的 Rhodes / Wurlitzer 电钢琴；加入微妙的爵士合成器铺底
吉他： 放克“鸡抓式”节奏（使用哇音踏板）；带有推弦/滑音的布鲁斯主音
管乐： 副歌后段加入管乐断奏（萨克斯 / 小号）
人声： 气声 R&B，配以福音风格的分层和声；散落的即兴 ad-libs
结构： 前奏（布鲁斯风格琶音 + 复古放克贝斯）→ 主歌（干声人声、电钢琴、沙锤、贝斯）→ 副歌（全套鼓、管乐、厚重和声）→ 桥段（精简：电钢琴、贝斯、合成器垫；自由人声）→ 尾奏（萨克斯/吉他对话，渐弱至沙锤/电钢琴循环）
温暖、模拟感的黑胶质感
空间对比： 干声主人声 vs. 宽阔的铺底音色
### 英文
#### 英文字符严格限制在1000英文字符以内，删除无用字符，如空格，换行，tab等
Vocals: low-pitched, raspy, weathered voice – Blues/R&B/Funk fusion, moody late-night bar vibe: restrained, groovy, melancholic yet rhythmic.Drums: brushed snare and rim clicks in verses; tight backbeat in choruses.Bass: funk slap/pop technique in verses; walking bass in choruses.Shaker/tambourine: throughout.Keyboards: Rhodes/Wurlitzer electric piano with extended chords (9ths, 13ths) and blue note clusters; subtle jazz synth pads.Guitar: funk "chicken scratch" rhythm (with wah pedal); blues lead with bends/slides.Horns: horn stabs (sax/trumpet) in the post-chorus.Vocals: breathy R&B with gospel-style layered harmonies; scattered ad-libs.Structure: Intro (bluesy arpeggios + retro funk bass) → Verse (dry vocals, Rhodes, shaker, bass) → Chorus (full drums, horns, thick harmonies) → Bridge (stripped-down: Rhodes, bass, synth pad; free vocals) → Outro (sax/guitar dialogue fading to shaker/Rhodes loop).Warm, analog vinyl texture.Spatial contrast: dry lead vocals vs. wide pads.
