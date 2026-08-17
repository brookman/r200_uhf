# Feature coverage vs R200 protocol (V2.3.3)

Device functions per the producer document *R200 user protocol V2.3.3.pdf*
(verified against the MagicRF M100 & QM100 firmware manual), mapped to this
crate. All `Yes` entries are verified on live hardware (M100, fw V2.3.5).

| Code | Function (per PDF) | Implemented | Crate method |
|------|--------------------|-------------|--------------|
| 0x03 | Get reader/writer module information | Yes | `get_module_info` |
| 0x22 | Single polling instruction | Yes | `single_polling_instruction` |
| 0x27 | Multiple polling instructions | Yes | `multi_polling_instruction` |
| 0x28 | Stop multiple polling instructions | Yes | `stop_multiple_polling_instructions` |
| 0x0C | Set Select parameter instruction | Yes | `select_tag` / `clear_select` |
| 0x0B | Get Select parameter instruction | No | |
| 0x12 | Set Send Select instruction (select mode) | Yes | `select_tag` / `clear_select` |
| 0x39 | Read label data storage area | Yes | `read_epc` / `read_mem` |
| 0x49 | Write label data storage area | Yes | `write_epc` / `write_mem` / `write_epc_reliable` |
| 0x82 | Lock label data store | Yes | `lock_tag` |
| 0x65 | Kill tag | Yes | `kill_tag` |
| 0x0D | Get Query parameters | No | |
| 0x0E | Set Query parameters | No | |
| 0x07 | Set work area (region) | Yes | `set_working_area` |
| 0x08 | Get work area (region) | Yes | `get_working_area` |
| 0xAB | Set working channel | No | |
| 0xAA | Get working channel | Yes | `get_working_channel` |
| 0xAD | Set automatic frequency hopping | No | |
| 0xA9 | Insert working channel | No | |
| 0xB6 | Set transmit power | Yes | `set_transmission_power` |
| 0xB7 | Acquire transmit power | Yes | `get_transmit_power` |
| 0xB0 | Set to transmit continuous carrier | No | |
| 0xF0 | Set receiving demodulator parameters | No | |
| 0xF1 | Get receiver demodulator parameters | No | |
| 0xF2 | Test RF input blocking signal | No | |
| 0xF3 | Test channel RSSI | No | |
| 0x11 | Set communication baud rate | No | |
| 0x1A | Control IO port | No | |
| 0x17 | Module sleep | No | |
| 0x1D | Set module idle sleep time | No | |
| 0x04 | Module IDLE mode | No | |
| 0xE0 | NXP ChangeConfig directive | No | |
| 0xE1 | NXP ReadProtect/ResetReadProtect | No | |
| 0xE3 | NXP ChangeEAS instruction | No | |
| 0xE4 | NXP EAS-Alarm instruction | No | |
| 0xE5/0xE6 | Impinj Monza4 QT instruction | No | |
| 0xD3/0xD4 | BlockPermlock instruction | No | |

Implemented: 17 / 37.
