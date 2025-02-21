include $(TOPDIR)/rules.mk

PKG_NAME:=ipv6-neigh
PKG_VERSION:=0.1.0
PKG_RELEASE:=1

PKG_SOURCE:=$(PKG_NAME)-$(PKG_VERSION).tar.gz
PKG_SOURCE_PROTO:=git
PKG_SOURCE_URL:=https://github.com/yourusername/ipv6-neigh.git
PKG_SOURCE_VERSION:=HEAD
PKG_SOURCE_DATE:=2025-02-20

PKG_MAINTAINER:=Your Name <your.email@example.com>
PKG_LICENSE:=MIT
PKG_LICENSE_FILES:=LICENSE

include $(INCLUDE_DIR)/package.mk
include $(INCLUDE_DIR)/rust-package.mk

define Package/ipv6-neigh
  SECTION:=net
  CATEGORY:=Network
  TITLE:=IPv6 neighbor display utility
  DEPENDS:=+libc
endef

define Package/ipv6-neigh/description
  A Rust implementation of 'ip -6 neigh' functionality for displaying IPv6 neighbors
endef

define Package/ipv6-neigh/install
	$(INSTALL_DIR) $(1)/usr/bin
	$(INSTALL_BIN) $(PKG_INSTALL_DIR)/usr/bin/ipv6-neigh $(1)/usr/bin/
endef

$(eval $(call BuildPackage,ipv6-neigh))
