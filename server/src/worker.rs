use std::thread;
use std::time::Duration;

use tauri::async_runtime::block_on;
use tauri::AppHandle;
use tauri::Emitter as _;
use tracing::debug;
use tracing::error;
use tracing::info;
use tracing::trace;
use xcap::image::ImageBuffer;
use xcap::image::Rgba;

use crate::chess;
use crate::common;
use crate::engine::QueryResult;
use crate::listen::ListenWindow;
use crate::listen::Window;
use crate::yolo::predict;
use crate::yolo::IMAGE_HEIGHT;
use crate::yolo::IMAGE_WIDTH;
use crate::SHARED_STATE;

// 棋盘分析结果
struct BoardAnalysisResult {
    expect_move: chess::Changed,
    expect_board: [[char; 9]; 10],
}

// 定义不同的棋盘状态
#[derive(PartialEq)]
enum ChessboardState {
    Initial,      // 初始状态，没有进行任何分析
    #[allow(dead_code)]
    StartPos,     // 初始棋盘状态
    OurTurn,      // 我方行棋
    OpponentTurn, // 对方行棋
    Invalid,      // 无效状态
}

// 分析上下文，保存分析状态和共享数据
struct AnalysisContext {
    app: AppHandle,
    window: ListenWindow,
    last_board: [[char; 9]; 10],
    expect_move: chess::Changed,
    expect_board: [[char; 9]; 10],
    invalid_change_count: usize,
}

unsafe impl Send for AnalysisContext {}
unsafe impl Sync for AnalysisContext {}

impl AnalysisContext {
    fn new(app: AppHandle, window: ListenWindow) -> Self {
        Self {
            app,
            // state_for_thread: state,
            window,
            last_board: [[' '; 9]; 10],
            expect_move: chess::Changed::default(),
            expect_board: [[' '; 9]; 10],
            invalid_change_count: 0,
        }
    }

    // 检查是否需要终止分析线程
    fn should_stop(&self) -> bool {
        let state = SHARED_STATE.get().unwrap();
        state.listen_thread.lock().unwrap().is_none()
    }

    // 获取棋盘图像并分析
    fn capture_and_analyze_board(&self) -> Option<(chess::Camp, [[char; 9]; 10])> {
        let image = self.window.capture()?;
        get_board(image)
    }

    // 确认棋盘状态是否稳定
    fn confirm_board(&self, board: [[char; 9]; 10]) -> bool {
        thread::sleep(Duration::from_millis(100));
        let conf_image = match self.window.capture() {
            Some(img) => img,
            None => return false,
        };
        if let Some((_, conf_board)) = get_board(conf_image) {
            return conf_board == board;
        }
        false
    }

    // 分析棋盘并返回结果
    fn analyze_board(&mut self, camp: &chess::Camp, board: [[char; 9]; 10]) -> Option<BoardAnalysisResult> {
        let fen = chess::board_fen(camp, board);
        let config = SHARED_STATE.get().unwrap().config.read().unwrap();
        let state = SHARED_STATE.get().unwrap();
        let mut engine = state.engine.lock().unwrap();
        let result = block_on(engine.search(&fen, &config.engine));
        result.as_ref()?;

        let (expect_move, expect_board) = analyse(&self.app, result.unwrap(), board);
        Some(BoardAnalysisResult { expect_move, expect_board })
    }

    // 更新UI显示
    fn update_ui(&self, camp: &chess::Camp, board: [[char; 9]; 10]) {
        let board_map = chess::board_map(board);
        let _ = self.app.emit("mirror", camp.is_black());
        let _ = self.app.emit("position", &board_map);
    }

    // 处理移动事件
    fn handle_move(&mut self, changed: &chess::Changed) { let _ = self.app.emit("move", changed); }

    // 处理错误变化计数
    fn handle_invalid_change(
        &mut self, last_board: [[char; 9]; 10], board: [[char; 9]; 10], camp: &chess::Camp,
    ) -> ChessboardState {
        if self.invalid_change_count < 3 {
            self.invalid_change_count += 1;
            let last_fen = chess::board_fen(camp, last_board);
            let current = chess::board_fen(camp, board);
            debug!("OneChanged last {}", last_fen);
            debug!("OneChanged current {}", current);
            ChessboardState::Invalid
        } else {
            // 如果出现次数超过3次，重置为初始状态
            debug!("OneChanged count=3, reload");
            self.invalid_change_count = 0;
            ChessboardState::Initial
        }
    }
}

pub fn get_board(image: ImageBuffer<Rgba<u8>, Vec<u8>>) -> Option<(chess::Camp, [[char; 9]; 10])> {
    if let Ok(data) = predict(image) {
        if let Ok((camp, mut board)) = common::detections_to_board(&data) {
            chess::board_fix(&camp, &mut board);
            return Some((camp, board));
        }
    }
    None
}

pub fn analyse(app: &AppHandle, mut result: QueryResult, board: [[char; 9]; 10]) -> (chess::Changed, [[char; 9]; 10]) {
    // 引擎结果翻译为中文
    let Some(best_pv) = result.pvs.first().cloned() else {
        info!("分析无有效走法 {:?}", result);
        let _ = app.emit("analyse", result);
        return (chess::Changed::default(), board);
    };
    let best_move = chess::board_move_chinese(board, &best_pv);
    let expect_board = chess::board_move(board, &best_pv);
    let expect_move = chess::Changed::from_pv(&best_pv, board);

    let mut tmp_board = expect_board;
    result.moves.push(best_move);
    for pv in result.pvs.iter().skip(1).take(3) {
        let mv = chess::board_move_chinese(tmp_board, pv);
        result.moves.push(mv);
        tmp_board = chess::board_move(tmp_board, pv);
    }
    // 把结果发送给前端
    info!("分析结果 {:?}", result);
    let _ = app.emit("analyse", result);

    // 返回一个预期move和预期board
    (expect_move, expect_board)
}

// 处理循环逻辑的主函数
fn process_analysis_loop(mut context: AnalysisContext) {
    let mut current_state = ChessboardState::Initial;

    loop {
        // 检查是否需要停止监听
        if context.should_stop() {
            debug!("listen stopped");
            break;
        }

        // 获取等待间隔
        let interval = SHARED_STATE.get().unwrap().config.read().unwrap().timer_interval;
        thread::sleep(Duration::from_millis(interval));

        // 捕获并分析棋盘
        let board_result = context.capture_and_analyze_board();
        if board_result.is_none() {
            continue;
        }

        let (camp, board) = board_result.unwrap();
        trace!("{:?} {:?}", camp, board);

        // 根据不同状态处理棋盘
        current_state = match current_state {
            ChessboardState::Initial => {
                // 初始状态，做第一次分析
                debug!("首次启动");

                // 设置前端棋盘
                context.update_ui(&camp, board);
                context.last_board = board;

                if chess::startpos(board) {
                    if camp.eq(&chess::Camp::Red) {
                        debug!("初始棋盘，我方红先，立即分析");
                        if let Some(result) = context.analyze_board(&camp, board) {
                            context.expect_move = result.expect_move;
                            context.expect_board = result.expect_board;
                        }
                        ChessboardState::OurTurn
                    } else {
                        debug!("初始棋盘，对方红先，等待对方走棋");
                        ChessboardState::OpponentTurn
                    }
                } else {
                    debug!("中局启动，立即分析当前局面");
                    if let Some(result) = context.analyze_board(&camp, board) {
                        context.expect_move = result.expect_move;
                        context.expect_board = result.expect_board;
                    }
                    ChessboardState::OurTurn
                }
            }

            ChessboardState::StartPos => {
                // 判断棋盘是否仍然是初始棋盘
                if !chess::startpos(board) {
                    // 不再是初始棋盘，处理正常的棋局变化
                    if board == context.last_board {
                        ChessboardState::StartPos // 没有变化
                    } else {
                        // 有变化，更新UI并分析
                        let (changed, board_state) = chess::board_diff(context.last_board, board);

                        match board_state {
                            chess::BoardChangeState::Move => {
                                context.last_board = board;
                                context.handle_move(&changed);

                                if camp.eq(&changed.camp) {
                                    // 我方移动完毕，等待对方走棋
                                    ChessboardState::OpponentTurn
                                } else {
                                    // 对方移动完毕，需要分析我方走法
                                    if let Some(result) = context.analyze_board(&camp, board) {
                                        context.expect_move = result.expect_move;
                                        context.expect_board = result.expect_board;
                                    }
                                    ChessboardState::OurTurn
                                }
                            }
                            chess::BoardChangeState::One => {
                                context.handle_invalid_change(context.last_board, board, &camp)
                            }
                            chess::BoardChangeState::Unknown => {
                                debug!("棋局变化未知，重置上下文");
                                context.update_ui(&camp, board);
                                context.last_board = board;
                                ChessboardState::Initial
                            }
                        }
                    }
                } else if chess::Camp::Red.eq(&camp) {
                    // 仍然是初始棋盘，且我方先手
                    if context.last_board == board {
                        // 防止重复分析
                        ChessboardState::StartPos
                    } else {
                        // 设置前端棋盘
                        context.last_board = board;
                        context.update_ui(&camp, board);

                        // 调用引擎查询
                        if let Some(result) = context.analyze_board(&camp, board) {
                            context.expect_move = result.expect_move;
                            context.expect_board = result.expect_board;
                        }

                        ChessboardState::OurTurn
                    }
                } else {
                    // 对方先手，跳过分析
                    debug!("对方先手，跳过分析");
                    context.last_board = board;
                    context.update_ui(&camp, board);
                    ChessboardState::OpponentTurn
                }
            }

            ChessboardState::OurTurn | ChessboardState::OpponentTurn => {
                // 判断棋盘是否未发生变化
                if board == context.last_board {
                    debug!("棋盘未发生变化，跳过分析");
                    current_state // 保持当前状态
                } else if board == context.expect_board {
                    // 符合预期棋盘，跳过分析
                    debug!("棋盘为预期棋盘，跳过分析");
                    let expect_move = context.expect_move.clone();
                    let expect_board = context.expect_board;
                    context.last_board = expect_board;
                    context.expect_board = [[' '; 9]; 10]; // 清空已满足的预期棋盘
                    context.handle_move(&expect_move);

                    // 我方走完预期棋后进入等待对方回合
                    ChessboardState::OpponentTurn
                } else {
                    // 确认棋盘变化是否稳定
                    if !context.confirm_board(board) {
                        debug!("棋盘延迟确认失败");
                        let confirm_interval = SHARED_STATE.get().unwrap().config.read().unwrap().confirm_interval;
                        thread::sleep(Duration::from_millis(confirm_interval));
                        current_state // 保持当前状态
                    } else if !chess::board_check(board) {
                        // 检测棋盘是否有效
                        let debug_fen = chess::board_fen(&camp, board);
                        debug!("棋盘识别无效: {}", debug_fen);
                        current_state // 保持当前状态
                    } else {
                        // 处理正常棋盘变化
                        let (changed, board_state) = chess::board_diff(context.last_board, board);

                        match board_state {
                            chess::BoardChangeState::Move => {
                                context.last_board = board;
                                context.expect_board = [[' '; 9]; 10]; // 出现新招法，清空预期
                                context.handle_move(&changed);

                                if camp.eq(&changed.camp) {
                                    // 我方移动，进入对方思考
                                    debug!("我方移动, {} -> {}, 等待对方走棋", changed.from, changed.to);
                                    ChessboardState::OpponentTurn
                                } else {
                                    // 对方移动，进入我方分析
                                    debug!("对方移动, {} -> {}, 开始分析我方走法", changed.from, changed.to);
                                    if let Some(result) = context.analyze_board(&camp, board) {
                                        context.expect_move = result.expect_move;
                                        context.expect_board = result.expect_board;
                                    }
                                    ChessboardState::OurTurn
                                }
                            }
                            chess::BoardChangeState::One => {
                                context.handle_invalid_change(context.last_board, board, &camp)
                            }
                            chess::BoardChangeState::Unknown => {
                                debug!("棋局变化未知，重置上下文");
                                context.update_ui(&camp, board);
                                context.last_board = board;
                                ChessboardState::Initial
                            }
                        }
                    }
                }
            }

            ChessboardState::Invalid => {
                // 复位到初始状态，等待下一次有效的变化
                ChessboardState::Initial
            }
        };
    }
}

// 初始化Tauri的command处理
#[tauri::command]
pub async fn start_listen(app: AppHandle, target: Window) -> Result<(), String> {
    trace!("start_listen");
    let shared_state = SHARED_STATE.get().unwrap();
    let mut thread_guard = match shared_state.listen_thread.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    if thread_guard.is_some() {
        error!("current listen thread is running, please stop it first");
        return Err("已经在监听中".to_string());
    }

    // 初始化监听窗口模块
    let Some(mut window) = ListenWindow::new(&target, IMAGE_WIDTH, IMAGE_HEIGHT) else {
        return Err("未找到指定窗口".to_string());
    };
    let Some(image) = window.capture() else {
        return Err("截取窗口图像失败".to_string());
    };

    let image_h = image.height();
    let image_w = image.width();

    let Ok(detections) = predict(image) else {
        return Err("模型推理识别失败".to_string());
    };

    match common::detections_bound(image_w, image_h, &detections) {
        Ok((x, y, w, h)) => {
            window.set_sub_bound(x, y, w, h); // 设置窗口边界
        }
        Err(e) => {
            return Err(e); // 未识别到棋盘
        }
    }

    // 创建分析上下文
    let context = AnalysisContext::new(app.clone(), window);

    // 启动后台线程进行截图和处理
    let listen_thread = thread::spawn(move || {
        trace!("into thread");
        process_analysis_loop(context);
    });

    thread_guard.replace(listen_thread);

    Ok(())
}

#[tauri::command]
pub fn stop_listen() {
    info!("stop listen");
    let shared_state = SHARED_STATE.get().unwrap();
    if let Ok(mut state) = shared_state.listen_thread.lock() {
        if let Some(listen_thread) = state.take() {
            // 释放锁，停止后台线程
            debug!("释放锁，停止后台线程");
            drop(state);
            let _ = listen_thread.join();
        }
    }
    debug!("stoped");
}
