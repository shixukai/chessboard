<script setup lang="ts">
import { NDialogProvider } from "naive-ui";
import Analyse from "./components/Analyse.vue";
import Chessboard from "./components/Chessboard.vue";
import Toolbar from "./components/Toolbar.vue";
</script>

<template>
    <n-dialog-provider>
        <div class="app-workbench">
            <!-- 顶部现代工具栏 -->
            <Toolbar />

            <!-- 主工作区：左侧立体棋盘，右侧自适应分析面板 -->
            <div class="workbench-body">
                <Chessboard />
                <Analyse />
            </div>
        </div>
    </n-dialog-provider>
</template>

<style scoped>
.app-workbench {
    width: 100%;
    height: 100vh;
    display: flex;
    flex-direction: column;
    padding: 10px;
    box-sizing: border-box;
    background: #f1f5f9;
    overflow: hidden;
}

.workbench-body {
    position: relative;
    flex: 1;
    width: 100%;
    margin-top: 10px;
    min-height: 0;
}
</style>
