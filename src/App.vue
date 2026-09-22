<script setup lang="ts">
import { NDivider, NLayoutFooter, NDialogProvider } from "naive-ui";
import Analyse from "./components/Analyse.vue";
import Chessboard from "./components/Chessboard.vue";
import Toolbar from "./components/Toolbar.vue";
</script>

<template>
    <n-dialog-provider>
        <!-- 最上面 -->
        <Toolbar />

        <n-divider class="spliter-toolbar" />

        <!-- 下方左侧 -->
        <Chessboard />

        <n-divider vertical class="spliter-middle" />

        <!-- 下方右侧 -->
        <Analyse />
    </n-dialog-provider>
</template>

<style scoped>
.spliter-toolbar {
    position: absolute;
    width: calc(100% - 20px);
    top: 60px;
    left: 10px;
    z-index: 1;
}

.spliter-middle {
    position: absolute;
    left: calc(20px + 380px * var(--board-scale, 1));
    top: 100px;
    height: calc(100% - 110px);
    min-height: 425px;
    z-index: 1;
}
</style>
