#ifndef LOG_H
#define LOG_H

#define LOG_PATH "/data/opbatt/battchg.log"

/* 获取时间戳字符串 "[YYYY-MM-DD-HH:MM:SS]" */
void get_timestamp(char *buf, int bufsz);

/* 写日志到 /data/opbatt/battchg.log 和 stdout */
void log_write(const char *msg);

#endif /* LOG_H */
