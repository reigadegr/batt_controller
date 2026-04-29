pub const TEMP_RANGE_MAX: usize = 5;
pub const CV_STEP_MAX: usize = 8;
pub const CONFIG_PATH: &str = "/data/opbatt/batt_control";

#[derive(Clone)]
pub struct BattConfig {
    // 温控
    pub temp_range: [i32; TEMP_RANGE_MAX],
    pub temp_range_count: usize,
    pub temp_curr_offset: [i32; TEMP_RANGE_MAX],
    pub temp_curr_offset_count: usize,

    // 电流调节步长
    pub adjust_step: i32,
    pub inc_step: i32,
    pub dec_step: i32,

    // UFCS 快充
    pub max_ufcs_chg_reset_cc: i32,
    pub ufcs_reset_delay: i32,
    pub ufcs_max: i32,
    pub pps_max: i32,
    pub cable_override: i32,

    // UFCS/PPS SoC 监控区间
    pub ufcs_soc_mon: [i32; 2],
    pub ufcs_interval_ms: [i32; 2],
    pub pps_soc_mon: [i32; 2],
    pub pps_interval_ms: [i32; 2],

    // 主循环
    pub loop_interval_ms: i32,

    // 电池电压控制
    pub batt_vol_thr: [i32; 2],
    pub batt_vol_soc: [i32; 2],
    pub batt_con_soc: i32,

    // 上升阶段
    pub rise_quickstep_thr_mv: i32,
    pub rise_wait_thr_mv: i32,

    // CV 阶段
    pub cv_vol_mv: i32,
    pub cv_max_ma: i32,
    pub cv_step_mv: [i32; CV_STEP_MAX],
    pub cv_step_ma: [i32; CV_STEP_MAX],
    pub cv_step_count: usize,

    // TC 阶段
    pub tc_vol_thr_mv: i32,
    pub tc_thr_soc: i32,
    pub tc_full_ma: i32,
    pub tc_vol_full_mv: i32,

    // 充电完成
    pub curr_inc_wait_cycles: i32,
    pub batt_full_thr_mv: i32,

    // 重启 RISE 阶段
    pub restart_rise_step: i32,

    // 去极化阶段
    pub depol_pulse_ma: i32,
    pub depol_zero_ma: i32,
    pub depol_neg_step: i32,

    // 使能标志
    pub enabled: i32,
}

/// 逗号分隔的整数数组解析
fn parse_int_array(val: &str, arr: &mut [i32]) -> usize {
    let mut count = 0;
    for token in val.split(',') {
        if count >= arr.len() {
            break;
        }
        if let Ok(v) = token.trim().parse::<i32>() {
            arr[count] = v;
            count += 1;
        }
    }
    count
}

/// 从 key=value 行中提取 value（精确键匹配）
fn extract_value<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    if line.len() < key.len() + 1 {
        return None;
    }
    if !line.starts_with(key) {
        return None;
    }
    match line.as_bytes().get(key.len()) {
        Some(b'=') => Some(&line[key.len() + 1..]),
        _ => None,
    }
}

impl Default for BattConfig {
    fn default() -> Self {
        Self {
            temp_range: [0; TEMP_RANGE_MAX],
            temp_range_count: 0,
            temp_curr_offset: [0; TEMP_RANGE_MAX],
            temp_curr_offset_count: 0,
            adjust_step: 0,
            inc_step: 0,
            dec_step: 0,
            max_ufcs_chg_reset_cc: 0,
            ufcs_reset_delay: 0,
            ufcs_max: 0,
            pps_max: 0,
            cable_override: 0,
            ufcs_soc_mon: [0; 2],
            ufcs_interval_ms: [0; 2],
            pps_soc_mon: [0; 2],
            pps_interval_ms: [0; 2],
            loop_interval_ms: 0,
            batt_vol_thr: [0; 2],
            batt_vol_soc: [0; 2],
            batt_con_soc: 0,
            rise_quickstep_thr_mv: 0,
            rise_wait_thr_mv: 0,
            cv_vol_mv: 0,
            cv_max_ma: 0,
            cv_step_mv: [0; CV_STEP_MAX],
            cv_step_ma: [0; CV_STEP_MAX],
            cv_step_count: 0,
            tc_vol_thr_mv: 0,
            tc_thr_soc: 0,
            tc_full_ma: 0,
            tc_vol_full_mv: 0,
            curr_inc_wait_cycles: 0,
            batt_full_thr_mv: 0,
            restart_rise_step: 0,
            depol_pulse_ma: 0,
            depol_zero_ma: 0,
            depol_neg_step: 0,
            enabled: 0,
        }
    }
}

impl BattConfig {
    /// 逐行解析 key=value 配置项
    fn apply_line(&mut self, line: &str) {
        // 用 let-chain 替代嵌套 if，消除 collapsible_if 警告
        macro_rules! parse_field {
            ($key:expr, $field:ident) => {
                if let Some(v) = extract_value(line, $key)
                    && let Ok(v) = v.parse()
                {
                    self.$field = v;
                }
            };
            ($key:expr, arr $field:ident) => {
                if let Some(v) = extract_value(line, $key) {
                    parse_int_array(v, &mut self.$field);
                }
            };
            ($key:expr, arr $field:ident, count $count:ident) => {
                if let Some(v) = extract_value(line, $key) {
                    self.$count = parse_int_array(v, &mut self.$field);
                }
            };
        }

        // 数组+计数字段
        parse_field!("temp_range", arr temp_range, count temp_range_count);
        parse_field!("temp_curr_offset", arr temp_curr_offset, count temp_curr_offset_count);
        parse_field!("cv_step_mv", arr cv_step_mv, count cv_step_count);
        if let Some(v) = extract_value(line, "cv_step_ma") {
            let ma_count = parse_int_array(v, &mut self.cv_step_ma);
            if self.cv_step_count > 0 && ma_count < self.cv_step_count {
                self.cv_step_count = ma_count;
            }
        }

        // 数组字段
        parse_field!("ufcs_soc_mon", arr ufcs_soc_mon);
        parse_field!("ufcs_interval_ms", arr ufcs_interval_ms);
        parse_field!("pps_soc_mon", arr pps_soc_mon);
        parse_field!("pps_interval_ms", arr pps_interval_ms);
        parse_field!("batt_vol_thr", arr batt_vol_thr);
        parse_field!("batt_vol_soc", arr batt_vol_soc);

        // 标量字段
        parse_field!("adjust_step", adjust_step);
        parse_field!("inc_step", inc_step);
        parse_field!("dec_step", dec_step);
        parse_field!("max_ufcs_chg_reset_cc", max_ufcs_chg_reset_cc);
        parse_field!("ufcs_reset_delay", ufcs_reset_delay);
        parse_field!("ufcs_max", ufcs_max);
        parse_field!("pps_max", pps_max);
        parse_field!("cable_override", cable_override);
        parse_field!("loop_interval_ms", loop_interval_ms);
        parse_field!("batt_con_soc", batt_con_soc);
        parse_field!("rise_quickstep_thr_mv", rise_quickstep_thr_mv);
        parse_field!("rise_wait_thr_mv", rise_wait_thr_mv);
        parse_field!("cv_vol_mv", cv_vol_mv);
        parse_field!("cv_max_ma", cv_max_ma);
        parse_field!("tc_vol_thr_mv", tc_vol_thr_mv);
        parse_field!("tc_thr_soc", tc_thr_soc);
        parse_field!("tc_full_ma", tc_full_ma);
        parse_field!("tc_vol_full_mv", tc_vol_full_mv);
        parse_field!("curr_inc_wait_cycles", curr_inc_wait_cycles);
        parse_field!("batt_full_thr_mv", batt_full_thr_mv);
        parse_field!("restart_rise_step", restart_rise_step);
        parse_field!("depol_pulse_ma", depol_pulse_ma);
        parse_field!("depol_zero_ma", depol_zero_ma);
        parse_field!("depol_neg_step", depol_neg_step);
        parse_field!("enabled", enabled);
    }

    /// 解析配置文件（key=value 格式），未覆盖的字段填充默认值
    pub fn parse(path: &str) -> Result<Self, std::io::Error> {
        let content = std::fs::read_to_string(path)?;
        // 硬编码默认值（对应 C 代码 config_parse 前半段）
        let mut cfg = Self {
            enabled: 1,
            adjust_step: 50,
            inc_step: 100,
            loop_interval_ms: 2000,
            restart_rise_step: 50,
            depol_pulse_ma: 500,
            depol_neg_step: 150,
            ufcs_soc_mon: [20, 60],
            ufcs_interval_ms: [450, 650],
            ..Default::default()
        };

        for line in content.lines() {
            let line = line.trim_end_matches('\r').trim();
            if line.is_empty() {
                continue;
            }
            cfg.apply_line(line);
        }

        // 补全未在配置文件中指定的字段默认值（对应 C 代码 config_parse 后半段）
        if cfg.cv_vol_mv == 0 {
            cfg.cv_vol_mv = 4450;
        }
        if cfg.cv_max_ma == 0 {
            cfg.cv_max_ma = 1000;
        }
        if cfg.tc_vol_thr_mv == 0 {
            cfg.tc_vol_thr_mv = 4500;
        }
        if cfg.tc_thr_soc == 0 {
            cfg.tc_thr_soc = 99;
        }
        if cfg.tc_full_ma == 0 {
            cfg.tc_full_ma = 200;
        }
        if cfg.tc_vol_full_mv == 0 {
            cfg.tc_vol_full_mv = 4530;
        }
        if cfg.batt_full_thr_mv == 0 {
            cfg.batt_full_thr_mv = 4530;
        }

        Ok(cfg)
    }

    /// 打印配置到 stdout（复现原始二进制输出格式）
    pub fn dump(&self) {
        println!("=== Initialize the CV configuration v1.8.9-OBF ===");

        print!("temp_range:");
        for i in 0..self.temp_range_count {
            print!(" {}", self.temp_range[i]);
        }
        println!();

        print!("temp_curr_offset:");
        for i in 0..self.temp_curr_offset_count {
            print!(" {}", self.temp_curr_offset[i]);
        }
        println!();

        println!("adjust_step: {}", self.adjust_step);
        println!("inc_step: {}", self.inc_step);
        println!("dec_step: {}", self.dec_step);

        println!(
            "batt_vol_thr: {} {}",
            self.batt_vol_thr[0], self.batt_vol_thr[1]
        );
        println!(
            "batt_vol_soc: {} {}",
            self.batt_vol_soc[0], self.batt_vol_soc[1]
        );
        println!("batt_con_soc: {}", self.batt_con_soc);

        println!("max_ufcs_chg_reset_cc: {}", self.max_ufcs_chg_reset_cc);
        println!("ufcs_max: {}", self.ufcs_max);
        println!("pps_max: {}", self.pps_max);

        println!(
            "ufcs_soc_mon: {} {}",
            self.ufcs_soc_mon[0], self.ufcs_soc_mon[1]
        );
        println!(
            "ufcs_interval_ms: {} {}",
            self.ufcs_interval_ms[0], self.ufcs_interval_ms[1]
        );
        println!(
            "pps_soc_mon: {} {}",
            self.pps_soc_mon[0], self.pps_soc_mon[1]
        );
        println!(
            "pps_interval_ms: {} {}",
            self.pps_interval_ms[0], self.pps_interval_ms[1]
        );

        println!("restart_rise_step: {}", self.restart_rise_step);
        println!("depol_pulse_ma: {}", self.depol_pulse_ma);
        println!("depol_zero_ma: {}", self.depol_zero_ma);
        println!("depol_neg_step: {}", self.depol_neg_step);

        println!("loop_interval_ms: {}", self.loop_interval_ms);
        println!("============================");
    }
}
