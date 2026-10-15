/*
 * 文件用途：pipe_protocol.h 的编解码与分行解析实现。
 */

#include "pipe_protocol.h"

#include <cstdlib>
#include <cstring>

namespace openkey {
namespace pipe {

std::string EncodeValue(const std::string& raw) {
  std::string out;
  out.reserve(raw.size() + 8);
  for (char c : raw) {
    switch (c) {
      case '\\': out += "\\\\"; break;
      case '\n': out += "\\n"; break;
      case ' ': out += "\\s"; break;
      default: out.push_back(c);
    }
  }
  return out;
}

std::string DecodeValue(const std::string& enc) {
  std::string out;
  out.reserve(enc.size());
  for (size_t i = 0; i < enc.size(); ++i) {
    if (enc[i] == '\\' && i + 1 < enc.size()) {
      switch (enc[i + 1]) {
        case '\\': out.push_back('\\'); ++i; break;
        case 'n': out.push_back('\n'); ++i; break;
        case 's': out.push_back(' '); ++i; break;
        default: out.push_back(enc[i]); break;  // 无法识别，保留反斜杠
      }
    } else {
      out.push_back(enc[i]);
    }
  }
  return out;
}

bool GetField(const FieldMap& fields, const char* key, std::string* out) {
  for (const auto& kv : fields) {
    if (kv.first == key) {
      *out = DecodeValue(kv.second);
      return true;
    }
  }
  return false;
}

bool GetFieldInt(const FieldMap& fields, const char* key, long long* out) {
  for (const auto& kv : fields) {
    if (kv.first == key) {
      *out = std::strtoll(kv.second.c_str(), nullptr, 10);
      return true;
    }
  }
  return false;
}

void SetField(FieldMap* fields, const char* key, const std::string& value) {
  std::string encoded = EncodeValue(value);
  for (auto& kv : *fields) {
    if (kv.first == key) {
      kv.second = encoded;
      return;
    }
  }
  fields->emplace_back(key, encoded);
}

void SetFieldInt(FieldMap* fields, const char* key, long long value) {
  SetField(fields, key, std::to_string(value));
}

std::string BuildRequestLine(const char* method, const FieldMap& fields) {
  std::string line = method ? method : "";
  for (const auto& kv : fields) {
    line.push_back(' ');
    line += kv.first;
    line.push_back('=');
    line += kv.second;  // value 在 SetField 时已编码
  }
  return line;
}

std::string BuildResponseLine(bool ok, const FieldMap& fields, int err_code,
                              const std::string& err_msg) {
  std::string line = ok ? "OK" : "ERR";
  if (ok) {
    for (const auto& kv : fields) {
      line.push_back(' ');
      line += kv.first;
      line.push_back('=');
      line += kv.second;
    }
  } else {
    line.push_back(' ');
    line += std::to_string(err_code);
    line.push_back(' ');
    line += EncodeValue(err_msg);
  }
  return line;
}

bool ParseLine(const std::string& line, std::string* first_token,
               FieldMap* fields) {
  fields->clear();
  size_t i = 0;
  // 跳过前导空格（容忍）
  while (i < line.size() && line[i] == ' ') ++i;

  // 首 token：至空格为止
  size_t start = i;
  while (i < line.size() && line[i] != ' ') ++i;
  if (i == start) return false;
  *first_token = line.substr(start, i - start);

  // 其余为 key=value（key 不含空格与等号）
  while (i < line.size()) {
    while (i < line.size() && line[i] == ' ') ++i;
    if (i >= line.size()) break;
    size_t key_start = i;
    while (i < line.size() && line[i] != ' ' && line[i] != '=') ++i;
    if (i == key_start) return false;  // 空 key，格式非法
    std::string key = line.substr(key_start, i - key_start);
    if (i < line.size() && line[i] == '=') {
      ++i;  // 跳过 '='
      size_t val_start = i;
      while (i < line.size() && line[i] != ' ') ++i;
      fields->emplace_back(key, line.substr(val_start, i - val_start));
    } else {
      // 无 '=' 的裸字段：视为空值
      fields->emplace_back(key, std::string());
    }
  }
  return true;
}

}  // namespace pipe
}  // namespace openkey
