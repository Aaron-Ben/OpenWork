// 在 ready 之前写一条错误并退出。
process.stderr.write("数据库连接失败\n");
process.exit(1);
