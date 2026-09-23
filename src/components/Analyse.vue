<script setup lang="ts">
import { listen } from '@tauri-apps/api/event';
import { NCard, NDivider, NFlex, NTag, NText, NScrollbar } from 'naive-ui';
import { ref, computed, nextTick } from 'vue';

interface Analyse {
    depth: number,   // 深度
    score: number,   // 得分
    time: number,    // 时间
    pvs: string[],   // 思考(iccs)
    moves: string[], // 思考(chinese)
    state: string,   // 状态
    source: string,  // 来源
}

interface StepLog {
    id: number;
    source: string;
    moves: string;
    score: number;
    depth: number;
}

const logs = ref<StepLog[]>([])
const best = ref({
    move: "等待思考...",
    depth: 0,
    score: 0,
    source: "",
})

const scrollbarRef = ref<any>(null);
let stepCounter = 0;

const formattedScore = computed(() => {
    if (!best.value.depth && best.value.move === "等待思考...") return "0.00";
    const cp = best.value.score;
    const val = (cp / 100).toFixed(2);
    return cp > 0 ? `+${val}` : val;
});

const scoreType = computed(() => {
    if (best.value.score > 50) return "success";
    if (best.value.score < -50) return "error";
    return "default";
});

listen('analyse', async (event) => {
    let data = event.payload as Analyse;
    let mvs = data.moves ? data.moves.join(" ") : "";
    
    stepCounter++;
    logs.value.push({
        id: stepCounter,
        source: data.source || "引擎",
        moves: mvs,
        score: data.score,
        depth: data.depth,
    });
    
    // 保持最大记录数，防止内存膨胀
    if (logs.value.length > 200) {
        logs.value.shift();
    }
    
    // 自动平滑滚动到底部
    nextTick(() => {
        scrollbarRef.value?.scrollTo({ position: 'bottom', silent: true });
    });

    best.value.move = data.moves && data.moves.length > 0 ? data.moves[0] : "----";
    best.value.depth = data.depth;
    best.value.score = data.score;
    best.value.source = data.source || "Pikafish";

    // 清理历史选择高亮
    document.querySelectorAll(".b-select").forEach((element) => {
        element.classList.remove("b-select");
    });

    // 设置新的推荐移动路径高亮 (起手与目标格)
    if (data.pvs && data.pvs.length > 0 && data.pvs[0].length >= 4) {
        let pv = data.pvs[0];
        let from = pv.substring(0, 2);
        let to = pv.substring(2, 4);
        document.getElementById(from)?.classList.add("b-select");
        document.getElementById(to)?.classList.add("b-select");
    }
});
</script>

<template>
    <n-card class="textlog modern-analyse-panel" :bordered="false" size="small">
        <!-- 顶部核心推荐指标看板 -->
        <div class="metrics-container">
            <n-flex align="center" justify="space-between" class="best-move-header">
                <span class="panel-section-title">推荐着法</span>
                <n-tag size="small" round :type="best.source.includes('云库') ? 'info' : 'primary'" class="source-tag">
                    {{ best.source || '本地引擎' }}
                </n-tag>
            </n-flex>
            
            <div class="best-move-display">
                <span class="best-move-text">{{ best.move }}</span>
            </div>

            <!-- 数据参数指标条 -->
            <div class="stat-pills-row">
                <div class="stat-pill">
                    <span class="stat-label">局势估分</span>
                    <n-tag :type="scoreType" size="small" round :bordered="false" class="stat-value-tag">
                        {{ formattedScore }}
                    </n-tag>
                </div>
                <div class="stat-pill">
                    <span class="stat-label">搜索深度</span>
                    <span class="stat-value-text">{{ best.depth > 0 ? `${best.depth}层` : '-' }}</span>
                </div>
            </div>
        </div>

        <n-divider class="analyse-divider" />

        <!-- 弹性自适应走棋记录列表 -->
        <div class="history-section">
            <div class="history-header">
                <span class="history-title">思考过程与走法日志</span>
                <span class="history-count" v-if="logs.length > 0">共 {{ logs.length }} 条</span>
            </div>
            
            <div class="history-scroll-wrapper">
                <n-scrollbar ref="scrollbarRef" class="custom-history-scrollbar">
                    <div class="history-list">
                        <div v-if="logs.length === 0" class="empty-hint">
                            暂无分析数据，请点击左上方“启动监听”
                        </div>
                        <div
                            v-for="item in logs"
                            :key="item.id"
                            class="history-row"
                        >
                            <span class="step-num">#{{ item.id }}</span>
                            <span class="step-source">[{{ item.source }}]</span>
                            <span class="step-content">{{ item.moves }}</span>
                        </div>
                    </div>
                </n-scrollbar>
            </div>
        </div>
    </n-card>
</template>

<style scoped>
.textlog {
    position: absolute;
    top: 72px;
    bottom: 14px;
    left: calc(24px + 380px * var(--board-scale, 1));
    right: 14px;
    width: auto;
    height: auto;
    min-width: 240px;
}

.modern-analyse-panel {
    background: #ffffff;
    border-radius: 12px;
    box-shadow: 0 4px 20px rgba(0, 0, 0, 0.04), 0 1px 3px rgba(0, 0, 0, 0.02);
    border: 1px solid rgba(0, 0, 0, 0.06);
    display: flex;
    flex-direction: column;
}

:deep(.n-card__content) {
    height: 100%;
    display: flex;
    flex-direction: column;
    padding: 16px !important;
}

.panel-section-title {
    font-size: 13px;
    font-weight: 600;
    color: #64748b;
    letter-spacing: 0.5px;
}

.best-move-display {
    margin: 8px 0;
    padding: 10px 14px;
    background: linear-gradient(135deg, rgba(24, 160, 88, 0.08) 0%, rgba(24, 160, 88, 0.02) 100%);
    border-radius: 8px;
    border: 1px solid rgba(24, 160, 88, 0.2);
    text-align: center;
}

.best-move-text {
    font-size: clamp(20px, calc(18px * var(--board-scale, 1)), 28px);
    font-weight: 700;
    color: #18a058;
    letter-spacing: 1.5px;
    font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Microsoft YaHei", sans-serif;
}

.stat-pills-row {
    display: flex;
    align-items: center;
    justify-content: space-around;
    gap: 8px;
    margin-top: 6px;
}

.stat-pill {
    display: flex;
    align-items: center;
    gap: 6px;
    background: #f8fafc;
    padding: 4px 10px;
    border-radius: 20px;
    border: 1px solid #e2e8f0;
}

.stat-label {
    font-size: 12px;
    color: #64748b;
}

.stat-value-text {
    font-size: 12px;
    font-weight: 600;
    color: #1e293b;
}

.analyse-divider {
    margin: 12px 0 !important;
}

.history-section {
    flex: 1;
    min-height: 0;
    display: flex;
    flex-direction: column;
}

.history-header {
    display: flex;
    justify-content: space-between;
    align-items: center;
    margin-bottom: 8px;
}

.history-title {
    font-size: 12px;
    font-weight: 600;
    color: #64748b;
}

.history-count {
    font-size: 11px;
    color: #94a3b8;
}

.history-scroll-wrapper {
    flex: 1;
    min-height: 0;
    background: #f8fafc;
    border-radius: 8px;
    border: 1px solid #f1f5f9;
    padding: 6px 4px;
}

.history-list {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 4px;
}

.empty-hint {
    padding: 30px 10px;
    text-align: center;
    color: #94a3b8;
    font-size: 13px;
}

.history-row {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 4px 8px;
    border-radius: 4px;
    transition: background 0.15s ease;
    font-size: clamp(12px, calc(11.5px * var(--board-scale, 1)), 15px);
    line-height: 1.6;
}

.history-row:hover {
    background: rgba(0, 0, 0, 0.04);
}

.step-num {
    font-size: 11px;
    font-weight: 600;
    color: #94a3b8;
    min-width: 24px;
    font-family: monospace;
}

.step-source {
    font-size: 11px;
    color: #3b82f6;
    font-weight: 500;
    white-space: nowrap;
}

.step-content {
    color: #1e293b;
    word-break: break-all;
    font-family: system-ui, -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, "Microsoft YaHei", sans-serif;
}
</style>