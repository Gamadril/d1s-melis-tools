# Allwinner F133/D1s Pin Mapping Description

Some info about device PIN mapping based on extracted `sys_config.fex` file.
To be updated during research.
Some of the settings might be left from default config - their presence in the config does not mean they are physically on the board.

## 1. Pin Configuration Syntax Reference

Allwinner FEX configuration maps pins using the following general pattern:
`port:<Port><Pin><Mux><Pull><Drive><Data>`

*   **Port**: GPIO Port Group (PA, PB, PC, PD, PE, PF, PG, PH, PL, PM, or POWER).
*   **Pin**: Port pin number (e.g., 00 to 31).
*   **Mux**: Multiplexor function mode (e.g., 1 = Input/Output GPIO, 2-7 = Alternate peripherals like UART, SPI, LCD, etc.).
*   **Pull**: Internal resistor configuration (`0` = High-impedance/Disable, `1` = Pull-up, `2` = Pull-down, `default` = Board default).
*   **Drive**: Drive strength level (`0` to `3`, `default` = Board default).
*   **Data**: Initial logic state for output GPIOs (`0` = Low, `1` = High, `default`).

---

## 2. Pin Assignments

### 2.1 System Debug & Communication Interfaces
debug logging, hardware control, and communication with the power management chip.

| Interface | Signal Name | Pin Assignment | Configuration Mode | Description |
| :--- | :--- | :--- | :--- | :--- |
| **System Debug UART** | `s_uart_tx` | **PL02** | `port:PL02<2><default><default><default>` | UART TX Debug Line |
| | `s_uart_rx` | **PL03** | `port:PL03<2><default><default><default>` | UART RX Debug Line |
| **JTAG (Debugging)** | `s_jtag_tms` | **PL04** | `port:PL04<2><1><2><default>` | JTAG Test Mode Select |
| | `s_jtag_tck` | **PL05** | `port:PL05<2><1><2><default>` | JTAG Test Clock |
| | `s_jtag_tdo` | **PL06** | `port:PL06<2><1><2><default>` | JTAG Test Data Out |
| | `s_jtag_tdi` | **PL07** | `port:PL07<2><1><2><default>` | JTAG Test Data In |
| **PMIC / RSB Bus** | `s_rsb_sck` | **PL00** | `port:PL00<2><1><2><default>` | Reduced Serial Bus Clock |
| | `s_rsb_sda` | **PL01** | `port:PL01<2><1><2><default>` | Reduced Serial Bus Data |
| **HDMI / DDC Bus** | `ddc_scl` | **PH13** | `port:PH13<3><default><1><default>` | Display Data Channel Clock |
| | `ddc_sda` | **PH14** | `port:PH14<3><default><1><default>` | Display Data Channel Data |
| | `cec_io` | **PH15** | `port:PH15<3><default><1><default>` | CEC Control Line |
| | `ddc_io_ctrl` | **PH02** | `port:PH02<1><default><default><0>` | HDMI DDC Enable Switch |

### 2.2 Storage Devices (SD Card / eMMC / NAND Flash)
SD card slots, onboard eMMC flash, and SPI NAND layouts.

| Interface | Signal Name | Pin Assignment | Configuration Mode | Description |
| :--- | :--- | :--- | :--- | :--- |
| **SDC0 (Main SD Card)**| `sdc_clk` | **PF02** | `port:PF02<2><1><2><default>` | SD Card Clock |
| | `sdc_cmd` | **PF03** | `port:PF03<2><1><2><default>` | SD Card Command |
| | `sdc_d0` to `sdc_d3` | **PF01**, **PF00**, **PF05**, **PF04** | `port:PFxx<2><1><2><default>` | SD Card Data Lines |
| **SDC1 (Secondary SD)**| `sdc_clk` | **PG00** | `port:PG00<2><1><2><default>` | Secondary SD Clock |
| | `sdc_cmd` | **PG01** | `port:PG01<2><1><2><default>` | Secondary SD Command |
| | `sdc_d0` to `sdc_d3` | **PG02**, **PG03**, **PG04**, **PG05** | `port:PGxx<2><1><2><default>` | Secondary SD Data Lines |
| **SDC2 / eMMC Interface**| `sdc_clk` | **PC05** | `port:PC05<3><1><2><default>` | eMMC Clock |
| | `sdc_cmd` | **PC06** | `port:PC06<3><1><2><default>` | eMMC Command |
| | `sdc_d0` to `sdc_d7` | **PC08** - **PC15** | `port:PCxx<3><1><2><default>` | 8-bit eMMC Data Lines |
| | `emmc_rst` | **PC16** | `port:PC16<3><1><2><default>` | eMMC Reset Line |
| **SDC3 (Tertiary SD)** | `sdc_clk` | **PA10** | `port:PA10<2><1><2><default>` | SD Slot 3 Clock |
| | `sdc_cmd` | **PA09** | `port:PA09<2><1><2><default>` | SD Slot 3 Command |
| | `sdc_d0` to `sdc_d3` | **PA11**, **PA12**, **PA13**, **PA14** | `port:PAxx<2><1><2><default>` | SD Slot 3 Data Lines |
| **NAND Flash Interface**| `nand0_ce1` | **PC15** | `port:PC15<2><1><1><default>` | NAND Chip Enable |
| | `nand0_rb1` | **PC16** | `port:PC16<2><1><1><default>` | NAND Ready/Busy Line |

### 2.3 LCD and Video Output
LCD and backlight.

| Signal Name | Pin Assignment | Configuration Mode | Description |
| :--- | :--- | :--- | :--- |
| `lcd_bl_en` | **PG15** | `port:PG15<1><0><default><1>` | LCD Backlight Enable |
| `lcd_power` | **POWER02** | `port:POWER02<1><0><default><1>` | LCD Panel Power VCC Enable |
| `lcd_gpio_0` to `21` | **PD00** to **PD21** | `port:PDxx<2><0><default><default>` | Parallel RGB Panel Pins |
| `vo_d0` to `15` | **PD01** to **PD17** | `port:PDxx<4><0><default><default>` | Alternate Digital Video Data (16-bit) |
| `vo_clk`, `vo_de`, `vo_hs`, `vo_vs` | **PD18**, **PD19**, **PD20**, **PD21** | `port:PDxx<4><0><default><default>` | Video Pixel Clock, Data Enable, HSync, VSync |

### 2.4 CSI / Camera Sensors
camera input interfaces and ISP.

| Signal Name | Pin Assignment | Configuration Mode | Description |
| :--- | :--- | :--- | :--- |
| `csi1_pck` | **PE00** | `port:PE00<2><default><default><default>` | CSI Pixel Clock |
| `csi1_hsync` | **PE02** | `port:PE02<2><default><default><default>` | CSI HSync |
| `csi1_vsync` | **PE03** | `port:PE03<2><default><default><default>` | CSI VSync |
| `csi1_d0` to `15` | **PE04** to **PE21** | `port:PExx<2><default><default><default>` | Parallel CSI Input Bus |
| `sensor0_reset` | **PE14** | `port:PE14<0><0><1><0>` | Camera Sensor 0 Reset Line |
| `sensor0_pwdn` | **PE16** | `port:PE16<0><0><1><0>` | Camera Sensor 0 Power-down |
| `sensor1_reset` | **PE14** | `port:PE14<0><0><1><0>` | Camera Sensor 1 Reset Line |
| `sensor1_pwdn` | **PE15** | `port:PE15<0><0><1><0>` | Camera Sensor 1 Power-down |

### 2.5 USB Controller Controls
| Signal Name | Pin Assignment | Configuration Mode | Description |
| :--- | :--- | :--- | :--- |
| `usb_id_gpio` | **PB06** | `port:PB06<0><0><default><default>` | USB OTG ID Sense pin |
| `usb_det_vbus_gpio` | **PB02** | `port:PB02<0><0><default><default>` | USB VBUS Voltage Detect line |
| `usb_drv_vbus_gpio` | **PB03** | `port:PB03<1><0><default><0>` | USB 5V VBUS Charge-pump Output Driver |
| `power_det_io` | **PB02** | `port:PB02<0><0><default><default>` | Power detection input |

### 2.6 Audio & Infrared Interfaces
| Signal Name | Pin Assignment | Configuration Mode | Description |
| :--- | :--- | :--- | :--- |
| `daudio1_mclk` | **PG11** | `port:PG11<2><0><1><default>` | Digital Audio Master Clock |
| `daudio1_bclk` | **PG13** | `port:PG13<2><0><1><default>` | Digital Audio Bit Clock |
| `daudio1_lrck` | **PG12** | `port:PG12<2><0><1><default>` | Digital Audio Frame Clock (WS) |
| `daudio1_dout0` | **PG15** | `port:PG15<2><0><1><default>` | Digital Audio Data Output |
| `daudio1_din0` | **PG14** | `port:PG14<2><0><1><default>` | Digital Audio Data Input |
| `gpio-spk` | **PH09** | `port:PH09<1><1><1><1>` | Audio Speaker Amplifier Enable (PA Shutdown) |
| `cir_pin` | **PB07** | `port:PB07<5><default><default><default>` | Consumer Infrared Receiver Input Pin |

### 2.7 Baseband Modem & Sensors
| Signal Name | Pin Assignment | Configuration Mode | Description |
| :--- | :--- | :--- | :--- |
| `bb_vbat` | **PL03** | `port:PL03<1><default><default><0>` | Baseband Power Enable |
| `bb_host_wake` | **PM00** | `port:PM00<1><default><default><0>` | Baseband Interrupt to Host |
| `bb_on` | **PM01** | `port:PM01<1><default><default><0>` | Baseband On Control |
| `bb_pwr_on` | **PM03** | `port:PM03<1><default><default><0>` | Baseband Power Pin |
| `bb_wake` | **PM04** | `port:PM04<1><default><default><0>` | Baseband Wakeup from Host |
| `bb_rf_dis` | **PM05** | `port:PM05<1><default><default><0>` | Baseband Radio Disable |
| `bb_rst` | **PM06** | `port:PM06<1><default><default><0>` | Baseband Hardware Reset |
| `gsensor_int1` | **PA09** | `port:PA09<6><1><default><default>` | Accelerometer/G-Sensor Interrupt |
| `gy_int1` | **PA10** | `port:PA10<6><1><default><default>` | Gyroscope Interrupt |
| `ls_int` | **PA12** | `port:PA12<6><1><default><default>` | Light/Proximity Sensor Interrupt |
| `compass_int` | **PA11** | `port:PA11<6><1><default><default>` | Compass Interrupt |

### 2.8 SPI & I2C (TWI) Interfaces

#### 2.8.1 SPI Controller (SPI0 / spinor_para)
CPU <-> SPI NOR Flash.

| Signal Name | Pin Assignment | Configuration Mode | Description |
| :--- | :--- | :--- | :--- |
| `spi_sclk` | **PC02** | `port:PC02<2><0><2><default>` | SPI Serial Clock |
| `spi_cs` | **PC03** | `port:PC03<2><1><2><default>` | SPI Chip Select |
| `spi0_mosi` | **PC04** | `port:PC04<2><0><2><default>` | SPI Master Out Slave In |
| `spi0_miso` | **PC05** | `port:PC05<2><0><2><default>` | SPI Master In Slave Out |
| `spi0_wp` | **PC06** | `port:PC06<2><0><2><default>` | SPI Write Protect |
| `spi0_hold` | **PC07** | `port:PC07<2><0><2><default>` | SPI Hold signal |

#### 2.8.2 TWI / I2C Buses
Power Management Unit (PMIC), touchscreen controllers, MFi.

| Interface | Signal Name | Pin Assignment | Configuration Mode | Description |
| :--- | :--- | :--- | :--- | :--- |
| **TWI0** | `twi0_scl` | **PB03** | `port:PB03<4><1><default><default>` | I2C-0 Clock Line |
| | `twi0_sda` | **PB02** | `port:PB02<4><1><default><default>` | I2C-0 Data Line |
| **TWI1** | `twi1_scl` | **PE00** | `port:PE00<4><1><default><default>` | I2C-1 Clock Line (connected to PMIC) |
| | `twi1_sda` | **PE01** | `port:PE01<4><1><default><default>` | I2C-1 Data Line (connected to PMIC) |
| **TWI2** | `twi2_scl` | **PE12** | `port:PE12<2><1><default><default>` | I2C-2 Clock Line (Sensors, Apple MFi co-CPU for CarPlay) |
| | `twi2_sda` | **PE13** | `port:PE13<2><1><default><default>` | I2C-2 Data Line (Sensors, Apple MFi co-CPU for CarPlay) |
| **TWI3** | `twi3_scl` | **PG07** | `port:PG07<4><1><default><default>` | I2C-3 Clock Line |
| | `twi3_sda` | **PG06** | `port:PG06<4><1><default><default>` | I2C-3 Data Line |

---

## 3. Onboard I2C / TWI Devices Address Map
Not confirmed

| Bus | Device Role | Slave Address (Decimal) | Slave Address (Hex) | Driver / Key Name |
| :--- | :--- | :--- | :--- | :--- |
| **TWI0** | Camera Sensor 0 CCI | `120` | `0x78` | `sensor0_twi_addr` |
| **TWI1** | Power Management Unit (PMIC) | `52` | `0x34` | `pmu_twi_addr` |
| **TWI1** | Capacitive Touch Panel (CTP) | `93` | `0x5d` | `ctp_twi_addr` |
| **TWI1** | Camera Sensor 1 CCI | `108` | `0x6c` | `sensor1_twi_addr` |
| **TWI2** | Compass | `13` | `0x0d` | `compass_twi_addr` |
| **TWI2** | Accelerometer (G-Sensor) | `24` | `0x18` | `gsensor_twi_addr` |
| **TWI2** | Light / Proximity Sensor | `35` | `0x23` | `ls_twi_addr` |
| **TWI2** | Gyroscope | `106` | `0x6a` | `gy_twi_addr` |
| **TWI2** | Apple MFi co-CPU (CarPlay) | *TBD* | *TBD* | Address pending (discover via logic analyzer) |
